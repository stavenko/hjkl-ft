import { describe, test, expect, beforeAll, afterAll } from "vitest";
import { Miniflare } from "miniflare";
import path from "node:path";

const WORKER_BUILD_DIR = path.resolve(__dirname, "..", "build", "worker");

let mf: Miniflare;

beforeAll(async () => {
  mf = new Miniflare({
    scriptPath: path.join(WORKER_BUILD_DIR, "shim.mjs"),
    modules: true,
    modulesRules: [{ type: "CompiledWasm", include: ["**/*.wasm"] }],
    durableObjects: {
      USER_DO: "UserDO",
    },
    bindings: {
      JWT_SECRET: "dev-secret-change-in-production",
    },
    compatibilityDate: "2024-01-01",
  });
});

afterAll(async () => {
  if (mf) {
    await mf.dispose();
  }
});

async function workerFetch(
  urlPath: string,
  init?: RequestInit,
): Promise<Response> {
  return mf.dispatchFetch(`http://localhost${urlPath}`, init);
}

describe("auth-worker integration", () => {
  test("unmatched route returns 404", async () => {
    const resp = await workerFetch("/");
    // The worker router has no GET / handler, so it should return 404
    expect(resp.status).toBe(404);
  });

  test("register/begin returns a challenge for a new user", async () => {
    const resp = await workerFetch("/register/begin", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ username: "testuser" }),
    });

    // The Durable Object should return a publicKey challenge
    expect(resp.status).toBe(200);

    const body = (await resp.json()) as Record<string, unknown>;
    expect(body).toHaveProperty("publicKey");

    const publicKey = body.publicKey as Record<string, unknown>;
    expect(publicKey).toHaveProperty("challenge");
    expect(publicKey).toHaveProperty("rp");
    expect(publicKey).toHaveProperty("user");
  });

  test("register/begin rejects missing body", async () => {
    const resp = await workerFetch("/register/begin", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: "{}",
    });

    // Missing "username" field should cause a parse/validation error
    // The worker returns 500 for RustError propagation
    expect(resp.status).toBeGreaterThanOrEqual(400);
  });

  test("token/validate rejects an invalid token", async () => {
    const resp = await workerFetch("/token/validate", {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: "Bearer invalid.token.value",
      },
    });

    expect(resp.status).toBe(401);

    const body = (await resp.json()) as { error: string };
    expect(body).toHaveProperty("error");
    expect(body.error).toBeTruthy();
  });

  test("token/validate rejects request without Authorization header", async () => {
    const resp = await workerFetch("/token/validate", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
    });

    expect(resp.status).toBe(401);

    const body = (await resp.json()) as { error: string };
    expect(body).toHaveProperty("error");
  });

  test("recovery/authenticate rejects wrong recovery key", async () => {
    const resp = await workerFetch("/recovery/authenticate", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        username: "nonexistent-user",
        recovery_key: "wrong-key-12345",
      }),
    });

    // No recovery key was ever set for this user, so verification fails
    expect(resp.status).toBe(401);

    const body = (await resp.json()) as { error: string };
    expect(body).toHaveProperty("error");
    expect(body.error).toContain("invalid recovery key");
  });

  test("add-device/begin rejects unauthenticated request", async () => {
    const resp = await workerFetch("/add-device/begin", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ username: "testuser" }),
    });

    expect(resp.status).toBe(401);

    const body = (await resp.json()) as { error: string };
    expect(body).toHaveProperty("error");
    expect(body.error).toContain("authentication required");
  });

  test("add-device/finish rejects unauthenticated request", async () => {
    const resp = await workerFetch("/add-device/finish", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        username: "testuser",
        credential: { id: "fake" },
      }),
    });

    expect(resp.status).toBe(401);

    const body = (await resp.json()) as { error: string };
    expect(body).toHaveProperty("error");
    expect(body.error).toContain("authentication required");
  });

  test("authenticate/begin for unregistered user returns error", async () => {
    const resp = await workerFetch("/authenticate/begin", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ username: "never-registered-user" }),
    });

    // A user with no credentials should fail at login begin
    // The exact status depends on the passkey-server library behavior
    expect(resp.status).toBeGreaterThanOrEqual(400);
  });
});
