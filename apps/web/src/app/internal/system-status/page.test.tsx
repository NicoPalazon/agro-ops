import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const auth = vi.hoisted(() => ({ accessToken: vi.fn() }));

vi.mock("@/lib/auth/server", () => ({
  authenticatedAccessToken: auth.accessToken,
}));

import SystemStatusPage from "./page";
import { BACKEND_REQUEST_TIMEOUT_MS } from "./status";

interface MockResponse {
  status?: number;
  body: unknown;
}

function mockBackend(responses: Record<string, MockResponse>) {
  const fetchMock = vi.fn(
    async (input: string | URL | Request, options?: RequestInit) => {
      if (options?.cache !== "no-store") {
        throw new Error("System Status requests must bypass the fetch cache");
      }

      const path = new URL(input.toString()).pathname;
      const response = responses[path];

      if (!response) {
        throw new Error(`No mock response for ${path}`);
      }

      return new Response(JSON.stringify(response.body), {
        status: response.status ?? 200,
        headers: { "content-type": "application/json" },
      });
    },
  );

  vi.stubGlobal("fetch", fetchMock);
  return fetchMock;
}

describe("System Status", () => {
  beforeEach(() => {
    auth.accessToken.mockResolvedValue("authenticated-access-token");
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.unstubAllEnvs();
  });

  it("renders successful service states", async () => {
    vi.stubEnv("API_BASE_URL", "http://api:8080");
    const fetchMock = mockBackend({
      "/internal/system-status": {
        body: {
          api: { status: "ok" },
          database: { status: "ready" },
          worker: {
            service: "worker",
            status: "healthy",
            last_seen_at: "2026-09-03T20:00:00.000000Z",
          },
          version: { service: "agro-ops-backend", version: "0.1.0" },
        },
      },
    });

    const markup = renderToStaticMarkup(await SystemStatusPage());

    expect(markup).toContain('data-service="web" data-status="operational"');
    expect(markup).toContain('data-service="api" data-status="operational"');
    expect(markup).toContain(
      'data-service="database" data-status="operational"',
    );
    expect(markup).toContain(
      'data-service="worker" data-status="operational"',
    );
    expect(markup).toContain("0.1.0");
    expect(fetchMock).toHaveBeenCalledOnce();
    for (const [input, options] of fetchMock.mock.calls) {
      expect(options).toMatchObject({ cache: "no-store" });
      expect(options?.signal).toBeInstanceOf(AbortSignal);
      expect(new URL(input.toString()).pathname).toBe("/internal/system-status");
      expect(options).toMatchObject({
        headers: { authorization: "Bearer authenticated-access-token" },
      });
    }
  });

  it("bounds every backend request", async () => {
    vi.stubEnv("API_BASE_URL", "http://api:8080");
    const timeout = vi.fn(() => new AbortController().signal);
    vi.stubGlobal("AbortSignal", { timeout });
    mockBackend({
      "/internal/system-status": {
        body: {
          api: { status: "ok" },
          database: { status: "ready" },
          worker: { service: "worker", status: "healthy", last_seen_at: null },
          version: { service: "agro-ops-backend", version: "0.1.0" },
        },
      },
    });

    await SystemStatusPage();

    expect(timeout).toHaveBeenCalledOnce();
    expect(timeout).toHaveBeenCalledWith(BACKEND_REQUEST_TIMEOUT_MS);
  });

  it("renders unavailable states when the backend cannot be reached", async () => {
    vi.stubEnv("API_BASE_URL", "http://api:8080");
    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new Error("offline")));

    const markup = renderToStaticMarkup(await SystemStatusPage());

    expect(markup).toContain('data-service="web" data-status="operational"');
    expect(markup).toContain('data-service="api" data-status="unavailable"');
    expect(markup).toContain(
      'data-service="database" data-status="unavailable"',
    );
    expect(markup).toContain(
      'data-service="worker" data-status="unavailable"',
    );
    expect(markup).toContain("Backend version</dt><dd>Unavailable");
  });

  it("does not display failed service checks as operational", async () => {
    vi.stubEnv("API_BASE_URL", "http://api:8080");
    mockBackend({
      "/internal/system-status": {
        body: {
          api: { status: "error" },
          database: { status: "not_ready" },
          worker: {
            service: "worker",
            status: "stale",
            last_seen_at: "2026-09-03T19:59:00.000000Z",
          },
          version: { service: "agro-ops-backend", version: "0.1.0" },
        },
      },
    });

    const markup = renderToStaticMarkup(await SystemStatusPage());

    expect(markup).toContain('data-service="api" data-status="degraded"');
    expect(markup).toContain(
      'data-service="database" data-status="degraded"',
    );
    expect(markup).toContain('data-service="worker" data-status="stale"');
    expect(markup).not.toContain(
      'data-service="api" data-status="operational"',
    );
    expect(markup).not.toContain(
      'data-service="worker" data-status="operational"',
    );
  });

  it("does not display a failed aggregate check as operational", async () => {
    vi.stubEnv("API_BASE_URL", "http://api:8080");
    mockBackend({
      "/internal/system-status": { status: 401, body: {} },
    });

    const markup = renderToStaticMarkup(await SystemStatusPage());

    expect(markup).toContain('data-service="api" data-status="degraded"');
    expect(markup).toContain('data-service="worker" data-status="degraded"');
    expect(markup).not.toContain(
      'data-service="worker" data-status="operational"',
    );
  });
});
