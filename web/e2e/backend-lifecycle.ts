import assert from "node:assert/strict";
import { type ChildProcess } from "node:child_process";
import { EventEmitter } from "node:events";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import {
  BackendFixtureOwner,
  type BackendInstance,
  useBackendFixture,
  waitForReady,
} from "./backend.ts";

class FakeProcess extends EventEmitter {
  exitCode: number | null = null;
  signalCode: NodeJS.Signals | null = null;
  pid: number | undefined = 4242;
  readonly signals: NodeJS.Signals[] = [];
  readonly failedSignals = new Set<NodeJS.Signals>();
  exitAfterSignal: NodeJS.Signals | undefined;

  kill(signal: NodeJS.Signals): boolean {
    this.signals.push(signal);
    if (this.failedSignals.has(signal)) {
      this.emit("error", new Error(`${signal} failed`));
      return false;
    }
    if (signal === this.exitAfterSignal) {
      queueMicrotask(() => {
        this.signalCode = signal;
        this.emit("exit", null, signal);
      });
    }
    return true;
  }
}

function child(fake: FakeProcess): ChildProcess {
  return fake as unknown as ChildProcess;
}

function fixtureInstance(cleanup: () => Promise<void>): BackendInstance {
  return {
    baseUrl: "https://fixture.invalid",
    tmpDir: "/unused",
    storeDir: "/unused",
    cleanup,
  };
}

test("partial fixture cleanup removes temporary state before process ownership", async () => {
  const dir = mkdtempSync(join(tmpdir(), "underwrap-e2e-owner-test-"));
  const owner = new BackendFixtureOwner(dir, 5);

  await owner.cleanup();

  assert.equal(existsSync(dir), false);
});

test("cleanup awaits SIGKILL exit before removing temporary state", async () => {
  const dir = mkdtempSync(join(tmpdir(), "underwrap-e2e-owner-test-"));
  const proc = new FakeProcess();
  proc.exitAfterSignal = "SIGKILL";
  const owner = new BackendFixtureOwner(dir, 5);
  owner.ownProcess(child(proc));

  await owner.cleanup();

  assert.deepEqual(proc.signals, ["SIGTERM", "SIGKILL"]);
  assert.equal(proc.signalCode, "SIGKILL");
  assert.equal(existsSync(dir), false);
});

test("spawn failure has no child to reap before temporary cleanup", async () => {
  const dir = mkdtempSync(join(tmpdir(), "underwrap-e2e-owner-test-"));
  const proc = new FakeProcess();
  proc.pid = undefined;
  const owner = new BackendFixtureOwner(dir, 5);
  owner.ownProcess(child(proc));

  await owner.cleanup();

  assert.deepEqual(proc.signals, []);
  assert.equal(existsSync(dir), false);
});

test("failed SIGTERM does not masquerade as child exit", async () => {
  const dir = mkdtempSync(join(tmpdir(), "underwrap-e2e-owner-test-"));
  const proc = new FakeProcess();
  proc.failedSignals.add("SIGTERM");
  proc.exitAfterSignal = "SIGKILL";
  const owner = new BackendFixtureOwner(dir, 5);
  owner.ownProcess(child(proc));

  await owner.cleanup();

  assert.deepEqual(proc.signals, ["SIGTERM", "SIGKILL"]);
  assert.equal(proc.signalCode, "SIGKILL");
  assert.equal(existsSync(dir), false);
});

test("failed synchronous signals retain temporary state", async () => {
  const dir = mkdtempSync(join(tmpdir(), "underwrap-e2e-owner-test-"));
  const proc = new FakeProcess();
  proc.failedSignals.add("SIGTERM");
  proc.failedSignals.add("SIGKILL");
  const owner = new BackendFixtureOwner(dir, 5);
  owner.ownProcess(child(proc));

  try {
    await assert.rejects(owner.cleanup(), /could not be sent SIGKILL/);
    assert.equal(existsSync(dir), true);
    assert.deepEqual(proc.signals, ["SIGTERM", "SIGKILL"]);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("cleanup retains temporary state when the child cannot be reaped", async () => {
  const dir = mkdtempSync(join(tmpdir(), "underwrap-e2e-owner-test-"));
  const proc = new FakeProcess();
  const owner = new BackendFixtureOwner(dir, 5);
  owner.ownProcess(child(proc));

  try {
    await assert.rejects(owner.cleanup(), /did not exit after SIGKILL/);
    assert.equal(existsSync(dir), true);
    assert.deepEqual(proc.signals, ["SIGTERM", "SIGKILL"]);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("fixture use failure still awaits cleanup and preserves the primary error", async () => {
  const primary = new Error("synthetic fixture use failure");
  let cleanupCalls = 0;

  await assert.rejects(
    useBackendFixture(
      async () => {
        throw primary;
      },
      {},
      async () =>
        fixtureInstance(async () => {
          cleanupCalls += 1;
          throw new Error("synthetic cleanup failure");
        }),
    ),
    (error) => error === primary,
  );
  assert.equal(cleanupCalls, 1);
});

test("one stalled readiness probe cannot outlive the startup deadline", async () => {
  const proc = new FakeProcess();
  const started = Date.now();

  await assert.rejects(
    waitForReady(
      "https://fixture.invalid",
      child(proc),
      20,
      async () => new Promise(() => {}),
    ),
    /did not become ready within 20ms/,
  );

  assert.ok(Date.now() - started < 500, "readiness deadline was not bounded");
});

test("child exit interrupts a stalled readiness probe", async () => {
  const proc = new FakeProcess();
  const readiness = waitForReady(
    "https://fixture.invalid",
    child(proc),
    5_000,
    async () => new Promise(() => {}),
  );
  queueMicrotask(() => {
    proc.exitCode = 17;
    proc.emit("exit", 17, null);
  });

  await assert.rejects(readiness, /exited \(code=17, signal=null\)/);
});
