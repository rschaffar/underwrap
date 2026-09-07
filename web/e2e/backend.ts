/**
 * Spawn a real `underwrap-backend` against an isolated temp directory.
 *
 * Each test gets its own backend so setup-guidance and network-rejection
 * tests start from an empty operator collection without interfering with
 * tests that rely on an existing admin. The cost is one process spawn
 * per test (~1 s), which is fine for the size of the e2e suite.
 *
 * The backend listens on a random localhost TLS port with a freshly-
 * generated self-signed cert. Playwright is configured with
 * `ignoreHTTPSErrors: true` so the browser will accept it. The SPA is
 * served by the embedded `rust_embed` table; in debug builds rust-embed
 * reads dynamically from the `UNDERWRAP_WEB_DIST` compile-time path
 * (defaulted to `<workspace>/web/dist` by `build.rs`), so a fresh
 * `npm run build` is enough between e2e runs — the daemon binary
 * itself doesn't have to be rebuilt.
 */
import { spawn, spawnSync, type ChildProcess } from "node:child_process";
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
import selfsigned from "selfsigned";

const __dirname = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(__dirname, "../..");

/** Where the cargo build drops the binary. Tests skip with a clear message if missing. */
function defaultBackendBinary(): string {
  return (
    process.env.UNDERWRAP_BACKENDD_BINARY ??
    join(REPO_ROOT, "target", "debug", "underwrap-backend")
  );
}

function builtSpaDir(): string {
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
  /** Absolute path of the pre-seeded default plain store directory. */
  storeDir: string;
  /** Tear down: SIGTERM the process, wait for exit, remove the temp dir. */
  cleanup: () => Promise<void>;
}

export interface StartBackendOptions {
  /** Provision an operator through the real offline CLI before the listener
   * starts so tests can jump straight to the `/login` UI. */
  withAdmin?: { username: string; password: string };
}

const BACKEND_STARTUP_TIMEOUT_MS = 15_000;
const BACKEND_SHUTDOWN_TIMEOUT_MS = 5_000;

function processExited(proc: ChildProcess): boolean {
  return proc.exitCode !== null || proc.signalCode !== null;
}

async function waitForProcessExit(
  proc: ChildProcess,
  timeoutMs: number,
): Promise<boolean> {
  if (processExited(proc)) return true;

  return new Promise<boolean>((resolveP) => {
    let settled = false;
    const finish = (exited: boolean) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      proc.removeListener("exit", onExit);
      proc.removeListener("error", onError);
      resolveP(exited);
    };
    const onExit = () => finish(true);
    // A spawn error means no child exists to reap.
    const onError = () => finish(true);
    const timer = setTimeout(() => finish(processExited(proc)), timeoutMs);
    proc.once("exit", onExit);
    proc.once("error", onError);
    if (processExited(proc)) finish(true);
  });
}

/** Owns a partially or fully started backend fixture until cleanup completes. */
export class BackendFixtureOwner {
  private proc: ChildProcess | undefined;
  private cleanupPromise: Promise<void> | undefined;
  private readonly tmpDir: string;
  private readonly shutdownTimeoutMs: number;

  constructor(
    tmpDir: string,
    shutdownTimeoutMs = BACKEND_SHUTDOWN_TIMEOUT_MS,
  ) {
    this.tmpDir = tmpDir;
    this.shutdownTimeoutMs = shutdownTimeoutMs;
  }

  ownProcess(proc: ChildProcess): void {
    if (this.proc) throw new Error("backend fixture process is already owned");
    this.proc = proc;
  }

  cleanup(): Promise<void> {
    this.cleanupPromise ??= this.cleanupOnce();
    return this.cleanupPromise;
  }

  private async cleanupOnce(): Promise<void> {
    const proc = this.proc;
    if (proc && !processExited(proc)) {
      proc.kill("SIGTERM");
      if (!(await waitForProcessExit(proc, this.shutdownTimeoutMs))) {
        proc.kill("SIGKILL");
        if (!(await waitForProcessExit(proc, this.shutdownTimeoutMs))) {
          throw new Error("underwrap-backend did not exit after SIGKILL");
        }
      }
    }
    rmSync(this.tmpDir, { recursive: true, force: true });
  }
}

export async function startBackend(
  opts: StartBackendOptions = {},
): Promise<BackendInstance> {
  const binary = defaultBackendBinary();
  if (!existsSync(binary) || !statSync(binary).isFile()) {
    throw new Error(
      `underwrap-backend binary not found at ${binary}. ` +
        "Run `cargo build -p underwrap-backend` (or set " +
        "UNDERWRAP_BACKENDD_BINARY) before `npm run test:e2e`.",
    );
  }
  const spaDir = builtSpaDir();
  if (!existsSync(spaDir) || !existsSync(join(spaDir, "index.html"))) {
    throw new Error(
      `Built SPA not found at ${spaDir}. Run \`npm run build\` first — ` +
        "the debug daemon reads it dynamically via rust-embed's debug mode.",
    );
  }

  const tmpDir = mkdtempSync(join(tmpdir(), "underwrap-e2e-"));
  const owner = new BackendFixtureOwner(tmpDir);
  try {
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

    const storeDir = join(tmpDir, "store");
    mkdirSync(storeDir);

    // Pre-seed a single "default" store in the backend's stores
    // registry. The registry is a directory of `<id>.json` files keyed
    // on a stable internal id; we write the one file before the daemon
    // starts so the first page render sees a store and doesn't
    // redirect through the "no stores visible" empty state.
    const storesDir = join(tmpDir, "stores");
    mkdirSync(storesDir);
    writeFileSync(
      join(storesDir, "default.json"),
      JSON.stringify({
        name: "default",
        description: "",
        backend: "plain",
        dir: storeDir,
      }),
    );

    // `stores_data_root` for the create-store path. The store CRUD
    // tests need this to be set to exercise the create flow; pointing
    // it at a per-test temp dir keeps the test isolated.
    const dataRootsDir = join(tmpDir, "data-root");
    mkdirSync(dataRootsDir);

    const configYaml = [
      `stores_dir: ${JSON.stringify(storesDir)}`,
      `clients_dir: ${JSON.stringify(join(tmpDir, "clients"))}`,
      `users_dir: ${JSON.stringify(join(tmpDir, "users"))}`,
      `bootstrap_file: ${JSON.stringify(join(tmpDir, "bootstrap.json"))}`,
      "https:",
      `  listen: "127.0.0.1:${port}"`,
      "  tls:",
      `    cert_file: ${JSON.stringify(certPath)}`,
      `    key_file: ${JSON.stringify(keyPath)}`,
      "admin:",
      "  allowed_origins:",
      `    - "${baseUrl}"`,
      `    - "https://127.0.0.1:${port}"`,
      `  stores_data_root: ${JSON.stringify(dataRootsDir)}`,
      "",
    ].join("\n");
    const configPath = join(tmpDir, "backend.yaml");
    writeFileSync(configPath, configYaml);

    const env: NodeJS.ProcessEnv = {
      ...process.env,
      // Stay quiet by default; flip to "debug" if a test hangs and you
      // need to see the backend's reasoning.
      RUST_LOG: process.env.UNDERWRAP_E2E_RUST_LOG ?? "warn",
    };
    if (opts.withAdmin) {
      const provision = spawnSync(
        binary,
        [
          "--config",
          configPath,
          "admin",
          "bootstrap-user",
          "--username",
          opts.withAdmin.username,
        ],
        {
          env,
          input: `${opts.withAdmin.password}\n`,
          encoding: "utf8",
        },
      );
      if (provision.status !== 0) {
        throw new Error(
          `offline bootstrap-user (${opts.withAdmin.username}) failed: ${provision.stderr}`,
        );
      }
    }

    const proc = spawn(binary, ["--config", configPath], {
      env,
      stdio: ["ignore", "pipe", "pipe"],
    });
    owner.ownProcess(proc);

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

    return {
      baseUrl,
      tmpDir,
      storeDir,
      cleanup: () => owner.cleanup(),
    };
  } catch (error) {
    try {
      await owner.cleanup();
    } catch {
      // Preserve the startup failure as the authoritative diagnostic.
    }
    throw error;
  }
}

export async function useBackendFixture(
  use: (instance: BackendInstance) => Promise<void>,
  opts: StartBackendOptions = {},
  start: (options: StartBackendOptions) => Promise<BackendInstance> = startBackend,
): Promise<void> {
  const instance = await start(opts);
  try {
    await use(instance);
  } catch (error) {
    try {
      await instance.cleanup();
    } catch {
      // Preserve the test failure as the authoritative diagnostic.
    }
    throw error;
  }
  await instance.cleanup();
}

type ReadinessProbe = (
  url: string,
  timeoutMs: number,
) => Promise<{ ok: boolean }>;

async function bounded<T>(promise: Promise<T>, timeoutMs: number): Promise<T> {
  let timer: NodeJS.Timeout | undefined;
  try {
    return await Promise.race([
      promise,
      new Promise<T>((_, rejectP) => {
        timer = setTimeout(
          () =>
            rejectP(new Error(`HTTPS readiness probe exceeded ${timeoutMs}ms`)),
          timeoutMs,
        );
      }),
    ]);
  } finally {
    if (timer) clearTimeout(timer);
  }
}

export async function waitForReady(
  baseUrl: string,
  proc: ChildProcess,
  timeoutMs = BACKEND_STARTUP_TIMEOUT_MS,
  probe: ReadinessProbe = (url, remainingMs) =>
    fetchInsecure(url, undefined, remainingMs),
): Promise<void> {
  const exitedMessage = () =>
    `underwrap-backend exited (code=${proc.exitCode}, signal=${proc.signalCode}) before becoming ready`;
  let terminationError: Error | undefined;
  let rejectTerminated: (error: Error) => void = () => {};
  const terminated = new Promise<never>((_, rejectP) => {
    rejectTerminated = rejectP;
  });
  const onExit = () => {
    terminationError = new Error(exitedMessage());
    rejectTerminated(terminationError);
  };
  const onError = (error: Error) => {
    terminationError = error;
    rejectTerminated(error);
  };
  proc.once("exit", onExit);
  proc.once("error", onError);

  const deadline = Date.now() + timeoutMs;
  let lastErr: unknown = null;
  try {
    while (Date.now() < deadline) {
      if (processExited(proc)) throw new Error(exitedMessage());
      const remainingMs = Math.max(1, deadline - Date.now());
      try {
        const res = await bounded(
          Promise.race([
            probe(`${baseUrl}/v1/admin/auth/bootstrap/status`, remainingMs),
            terminated,
          ]),
          remainingMs,
        );
        if (res.ok) return;
      } catch (err) {
        if (terminationError) throw terminationError;
        if (processExited(proc)) throw new Error(exitedMessage());
        lastErr = err;
      }
      const sleepMs = Math.min(100, Math.max(0, deadline - Date.now()));
      if (sleepMs > 0) await sleep(sleepMs);
    }
    throw new Error(
      `underwrap-backend at ${baseUrl} did not become ready within ${timeoutMs}ms: ${
        lastErr instanceof Error ? lastErr.message : String(lastErr)
      }`,
    );
  } finally {
    proc.removeListener("exit", onExit);
    proc.removeListener("error", onError);
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
  timeoutMs = BACKEND_STARTUP_TIMEOUT_MS,
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
        signal: AbortSignal.timeout(timeoutMs),
      },
      (res) => {
        const chunks: Buffer[] = [];
        res.on("data", (c) => chunks.push(c));
        res.on("error", rejectP);
        res.on("aborted", () =>
          rejectP(new Error("HTTPS readiness response aborted")),
        );
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
