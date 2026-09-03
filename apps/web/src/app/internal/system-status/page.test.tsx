import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import SystemStatusPage from "./page";

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
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.unstubAllEnvs();
  });

  it("renders successful service states", async () => {
    vi.stubEnv("API_BASE_URL", "http://api:8080");
    const fetchMock = mockBackend({
      "/health": { body: { status: "ok" } },
      "/ready": { body: { status: "ready" } },
      "/internal/worker/status": {
        body: {
          service: "worker",
          status: "healthy",
          last_seen_at: "2026-09-03T20:00:00.000000Z",
        },
      },
      "/version": {
        body: { service: "agro-ops-backend", version: "0.1.0" },
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
    expect(fetchMock).toHaveBeenCalledTimes(4);
    for (const [, options] of fetchMock.mock.calls) {
      expect(options).toMatchObject({ cache: "no-store" });
    }
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
      "/health": { status: 503, body: { status: "error" } },
      "/ready": { status: 503, body: { status: "not_ready" } },
      "/internal/worker/status": {
        body: {
          service: "worker",
          status: "stale",
          last_seen_at: "2026-09-03T19:59:00.000000Z",
        },
      },
      "/version": { status: 503, body: { status: "unavailable" } },
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
});
