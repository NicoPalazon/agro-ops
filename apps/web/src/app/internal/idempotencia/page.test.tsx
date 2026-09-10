import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const auth = vi.hoisted(() => ({ accessToken: vi.fn() }));

vi.mock("@/lib/auth/server", () => ({
  authenticatedAccessToken: auth.accessToken,
}));

import IdempotencyPage from "./page";

const baseRecord = {
  id: "01991d89-6a00-7000-8000-000000000001",
  operacion: "documento.crear",
  idempotency_key: "documento-2026-0001",
  request_sha256: "a".repeat(64),
  completado_en: "2026-09-09T14:00:00.000000Z",
  resultado_bytes: 42,
};

describe("Diagnóstico de idempotencia", () => {
  beforeEach(() => {
    auth.accessToken.mockResolvedValue("authenticated-access-token");
    vi.stubEnv("API_BASE_URL", "http://api:8080");
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.unstubAllEnvs();
  });

  it("renderiza metadatos seguros y etiquetas españolas sin resultados ni controles", async () => {
    const secret = "private-replay-result-secret";
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(
        new Response(
          JSON.stringify({ registros: [{ ...baseRecord, resultado: { secret } }] }),
          { status: 200 },
        ),
      ),
    );

    const markup = renderToStaticMarkup(await IdempotencyPage());

    for (const label of [
      "Operación",
      "Clave de idempotencia",
      "Huella de solicitud",
      "Fecha de finalización",
      "Tamaño del resultado",
    ]) {
      expect(markup).toContain(label);
    }
    expect(markup).toContain("documento.crear");
    expect(markup).toContain("42 bytes");
    expect(markup).not.toContain(secret);
    expect(markup).not.toContain("<button");
    expect(markup).not.toContain("Reintentar");
    expect(fetch).toHaveBeenCalledWith(
      expect.objectContaining({ pathname: "/internal/idempotencia" }),
      expect.objectContaining({
        cache: "no-store",
        headers: { authorization: "Bearer authenticated-access-token" },
      }),
    );
  });

  it("muestra un estado vacío estable", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(
        new Response(JSON.stringify({ registros: [] }), { status: 200 }),
      ),
    );

    const markup = renderToStaticMarkup(await IdempotencyPage());
    expect(markup).toContain("No hay registros de idempotencia");
    expect(markup).toContain("No hay comandos completados para mostrar.");
  });

  it("degrada sólo la página cuando falla el backend", async () => {
    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new Error("offline")));

    const markup = renderToStaticMarkup(await IdempotencyPage());
    expect(markup).toContain("No se pudo consultar la idempotencia");
    expect(markup).toContain('href="/internal"');
  });
});
