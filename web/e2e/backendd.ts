/**
 * Spawn a real `underwrap-backendd` against an isolated temp directory.
 *
 * Each test gets its own backend so the bootstrap-flow tests start
 * from an empty operator collection without interfering with the
 * tests that rely on an existing admin. The cost is one process spawn
 * per test (~1 s), which is fine for the size of the e2e suite.
 *
 * The backend listens on a random localhost TLS port with a freshly-
 * generated self-signed cert. Playwright is configured with
 * `ignoreHTTPSErrors: true` so the browser will accept it. The SPA is
 * served from `web/dist/` via `UNDERWRAP_BACKENDD_WEB_DIR`, exercising
 * the same single-origin layout we ship in production.
 */
import { spawn, type ChildProcess } from "node:child_process";
import { createServer } from "node:net";
import {
  mkdtempSync,
  mkdirSync,
  rmSync,
  writeFileSync,
  existsSync,
  statSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { randomBytes } from "node:crypto";
import selfsigned from "selfsigned";

const __dirname = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(__dirname, "../..");

/** Where the cargo build drops the binary. Tests skip with a clear message if missing. */
function defaultBackendBinary(): string {
  return (
    process.env.UNDERWRAP_BACKENDD_BINARY ??
    join(REPO_ROOT, "target", "debug", "underwrap-backendd")
  );
}

function defaultWebDir(): string {
  return join(REPO_ROOT, "web", "dist");
}

async function pickFreePort(): Promise<number> {
  return new Promise<number>((resolveP, rejectP) => {
    const srv = createServer();
    srv.unref();
    srv.on("error", rejectP);
    srv.listen(0, "127.0.0.1", () => {
      const addr = srv.address();
      if (typeof addr === "object" && addr !== null) {
        const { port } = addr;
        srv.close(() => resolveP(port));
      } else {
        srv.close();
        rejectP(new Error("could not determine listen port"));
      }
    });
  });
}

export interface BackendInstance {
  /** Base URL the SPA + admin API are served from, e.g. `https://localhost:48571`. */
  baseUrl: string;
  /** Absolute path of the per-test temp directory (config, certs, store live here). */
  tmpDir: string;
  /** Tear down: SIGTERM the process, wait for exit, remove the temp dir. */
  cleanup: () => Promise<void>;
}

export interface StartBackendOptions {
  /**
   * If true, bootstrap an admin operator before yielding so tests can
   * jump straight to the `/login` UI. Bootstrap is performed through
   * the public `POST /v1/admin/auth/bootstrap` endpoint, so the
   * sentinel and password hashing are exercised the same way as a
   * real first-time setup.
   */
  withAdmin?: { username: string; password: string };
}

export async function startBackend(
  opts: StartBackendOptions = {},
): Promise<BackendInstance> {
  const binary = defaultBackendBinary();
  if (!existsSync(binary) || !statSync(binary).isFile()) {
    throw new Error(
      `underwrap-backendd binary not found at ${binary}. ` +
        "Run `cargo build -p backend-backendd` (or set " +
        "UNDERWRAP_BACKENDD_BINARY) before `npm run test:e2e`.",
    );
  }
  const webDir = defaultWebDir();
  if (!existsSync(webDir) || !existsSync(join(webDir, "index.html"))) {
    throw new Error(
      `Built SPA not found at ${webDir}. Run \`npm run build\` first.`,
    );
  }

  const tmpDir = mkdtempSync(join(tmpdir(), "underwrap-e2e-"));
  const port = await pickFreePort();
  const baseUrl = `https://localhost:${port}`;

  const certPath = join(tmpDir, "cert.pem");
  const keyPath = join(tmpDir, "key.pem");
  // selfsigned >=5.x returns a Promise. The keys we use:
  //   cert: PEM-encoded certificate
  //   private: PEM-encoded private key
  const pems = await selfsigned.generate(
    [{ name: "commonName", value: "localhost" }],
    {
      days: 1,
      keySize: 2048,
      extensions: [
        {
          name: "subjectAltName",
          altNames: [
            { type: 2, value: "localhost" },
            { type: 7, ip: "127.0.0.1" },
          ],
        },
      ],
    },
  );
  writeFileSync(certPath, pems.cert);
  writeFileSync(keyPath, pems.private);

  const sessionKeyPath = join(tmpDir, "session.key");
  writeFileSync(sessionKeyPath, randomBytes(32));

  const storeDir = join(tmpDir, "store");
  mkdirSync(storeDir);

  const configYaml = [
    "stores:",
    "  default:",
    "    backend: plain",
    `    dir: ${JSON.stringify(storeDir)}`,
    "admin:",
    `  listen: "127.0.0.1:${port}"`,
    "  tls:",
    `    cert_file: ${JSON.stringify(certPath)}`,
    `    key_file: ${JSON.stringify(keyPath)}`,
    `  session_key_file: ${JSON.stringify(sessionKeyPath)}`,
    "  admin_home_store: default",
    "  allowed_origins:",
    `    - "${baseUrl}"`,
    `    - "https://127.0.0.1:${port}"`,
    "",
  ].join("\n");
  const configPath = join(tmpDir, "backend.yaml");
  writeFileSync(configPath, configYaml);

  const env: NodeJS.ProcessEnv = {
    ...process.env,
    UNDERWRAP_BACKENDD_WEB_DIR: webDir,
    // Stay quiet by default; flip to "debug" if a test hangs and you
    // need to see the backend's reasoning.
    RUST_LOG: process.env.UNDERWRAP_E2E_RUST_LOG ?? "warn",
  };
  const proc = spawn(binary, ["--config", configPath], {
    env,
    stdio: ["ignore", "pipe", "pipe"],
  });

  // Tee stderr to our stderr when the env var is set — useful when
  // debugging a test locally; silent in CI.
  if (process.env.UNDERWRAP_E2E_LOG_BACKEND) {
    proc.stdout?.on("data", (b: Buffer) => process.stderr.write(b));
    proc.stderr?.on("data", (b: Buffer) => process.stderr.write(b));
  }

  // Wait until the admin listener is accepting requests. We probe
  // the public bootstrap-status endpoint — it's GET, requires no
  // session, and returns 200 once the server is fully up.
  await waitForReady(baseUrl, proc);

  if (opts.withAdmin) {
    await bootstrapAdmin(baseUrl, opts.withAdmin);
  }

  const cleanup = async () => {
    if (!proc.killed && proc.exitCode === null) {
      proc.kill("SIGTERM");
      // Wait up to 5 s; if it doesn't exit, SIGKILL.
      const exited = await Promise.race([
        new Promise<boolean>((r) => proc.once("exit", () => r(true))),
        new Promise<boolean>((r) => setTimeout(() => r(false), 5_000)),
      ]);
      if (!exited) proc.kill("SIGKILL");
    }
    try {
      rmSync(tmpDir, { recursive: true, force: true });
    } catch {
      // best effort
    }
  };

  return { baseUrl, tmpDir, cleanup };
}

async function waitForReady(
  baseUrl: string,
  proc: ChildProcess,
): Promise<void> {
  const deadline = Date.now() + 15_000;
  let lastErr: unknown = null;
  while (Date.now() < deadline) {
    if (proc.exitCode !== null) {
      throw new Error(
        `underwrap-backendd exited (code=${proc.exitCode}) before becoming ready`,
      );
    }
    try {
      const res = await fetchInsecure(
        `${baseUrl}/v1/admin/auth/bootstrap/status`,
      );
      if (res.ok) return;
    } catch (err) {
      lastErr = err;
    }
    await sleep(100);
  }
  throw new Error(
    `underwrap-backendd at ${baseUrl} did not become ready within 15s: ${
      lastErr instanceof Error ? lastErr.message : String(lastErr)
    }`,
  );
}

async function bootstrapAdmin(
  baseUrl: string,
  creds: { username: string; password: string },
): Promise<void> {
  const res = await fetchInsecure(`${baseUrl}/v1/admin/auth/bootstrap`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      // The admin listener requires Origin on state-changing requests
      // (the CSRF-protection layer); the same-origin baseUrl satisfies
      // it without needing the cookie+header double-submit that only
      // applies to authenticated sessions.
      Origin: baseUrl,
    },
    body: JSON.stringify(creds),
  });
  if (!res.ok) {
    throw new Error(
      `bootstrap admin (${creds.username}) failed: ${res.status} ${await res.text()}`,
    );
  }
}

/**
 * Probe a URL over HTTPS with TLS verification disabled — the
 * fixture's self-signed cert isn't in any trust store. Implemented
 * with Node's built-in `https` module so we don't need a stand-alone
 * undici dependency (some Node distributions don't expose it as a
 * resolvable module name even though the fetch implementation uses
 * it internally).
 */
async function fetchInsecure(
  url: string,
  init?: { method?: string; headers?: Record<string, string>; body?: string },
): Promise<{ ok: boolean; status: number; text: () => Promise<string> }> {
  const https = await import("node:https");
  return new Promise((resolveP, rejectP) => {
    const u = new URL(url);
    const req = https.request(
      {
        method: init?.method ?? "GET",
        hostname: u.hostname,
        port: u.port,
        path: u.pathname + u.search,
        headers: init?.headers,
        rejectUnauthorized: false,
      },
      (res) => {
        const chunks: Buffer[] = [];
        res.on("data", (c) => chunks.push(c));
        res.on("end", () => {
          const status = res.statusCode ?? 0;
          resolveP({
            ok: status >= 200 && status < 300,
            status,
            text: async () => Buffer.concat(chunks).toString("utf8"),
          });
        });
      },
    );
    req.on("error", rejectP);
    if (init?.body) req.write(init.body);
    req.end();
  });
}

function sleep(ms: number): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}
