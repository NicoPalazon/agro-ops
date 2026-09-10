import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const auth = vi.hoisted(() => ({ accessToken: vi.fn() }));

vi.mock("@/lib/auth/server", () => ({
  authenticatedAccessToken: auth.accessToken,
}));

import OutboxPage from "./page";

const baseEvent = {
  id: "01991d89-6a00-7000-8000-000000000001",
  destino: "test_adapter",
  evento_tipo: "test.delivery_requested",
  entidad_tipo: "test.entity",
  entidad_id: "01991d89-6a00-7000-8000-000000000002",
  referencia: "REF-2026-0001",
  idempotency_key: "delivery-stable-key",
  ocurrido_en: "2026-09-09T14:00:00.000000Z",
  creado_en: "2026-09-09T14:00:00.000000Z",
  intentos: 1,
  max_intentos: 3,
  next_attempt_at: "2026-09-09T15:00:00.000000Z",
  bloqueado_en: null,
  bloqueado_por: null,
  actualizado_en: "2026-09-09T14:30:00.000000Z",
  completado_en: null,
  ultimo_error: "Resumen seguro",
};

describe("Diagnóstico del outbox", () => {
  beforeEach(() => {
    auth.accessToken.mockResolvedValue("authenticated-access-token");
    vi.stubEnv("API_BASE_URL", "http://api:8080");
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.unstubAllEnvs();
  });

  it("renderiza filas normales y etiquetas españolas sin payload ni controles", async () => {
    const secret = "private-payload-secret";
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(
        new Response(
          JSON.stringify({
            eventos: ["pendiente", "ejecutando", "completado", "agotado"].map(
              (estado, index) => ({
                ...baseEvent,
                id: `${baseEvent.id.slice(0, -1)}${index + 1}`,
                estado,
                payload: { authorization: secret },
              }),
            ),
          }),
          { status: 200 },
        ),
      ),
    );

    const markup = renderToStaticMarkup(await OutboxPage());

    for (const label of ["Pendiente", "Ejecutando", "Completado", "Agotado"]) {
      expect(markup).toContain(label);
    }
    expect(markup).toContain("test_adapter");
    expect(markup).toContain("test.delivery_requested");
    expect(markup).toContain("REF-2026-0001");
    expect(markup).toContain("Resumen seguro");
    expect(markup).not.toContain(secret);
    expect(markup).not.toContain("authorization");
    expect(markup).not.toContain("<button");
    expect(markup).not.toContain("Reintentar");
    expect(fetch).toHaveBeenCalledWith(
      expect.objectContaining({ pathname: "/internal/outbox" }),
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
        new Response(JSON.stringify({ eventos: [] }), { status: 200 }),
      ),
    );

    const markup = renderToStaticMarkup(await OutboxPage());
    expect(markup).toContain("No hay eventos de integración");
    expect(markup).toContain("El outbox transaccional está vacío.");
  });

  it("degrada sólo la página cuando falla el backend", async () => {
    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new Error("offline")));

    const markup = renderToStaticMarkup(await OutboxPage());
    expect(markup).toContain("No se pudo consultar el outbox");
    expect(markup).toContain(
      "El diagnóstico de integraciones no está disponible temporalmente.",
    );
    expect(markup).toContain('href="/internal"');
  });
});
