import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const auth = vi.hoisted(() => ({ accessToken: vi.fn() }));

vi.mock("@/lib/auth/server", () => ({
  authenticatedAccessToken: auth.accessToken,
}));

import AuditPage from "./page";

const baseEvent = {
  id: "01991d89-6a00-7000-8000-000000000001",
  ocurrido_en: "2026-09-09T14:00:00.000000Z",
  accion: "usuario.actualizado",
  entidad_tipo: "usuario",
  entidad_id: "01991d89-6a00-7000-8000-000000000002",
  referencia: "Usuario técnico",
  actor_tipo: "usuario",
  actor_usuario_id: "01991d89-6a00-7000-8000-000000000003",
  tiene_estado_anterior: true,
  tiene_estado_posterior: true,
};

describe("Diagnóstico de auditoría", () => {
  beforeEach(() => {
    auth.accessToken.mockResolvedValue("authenticated-access-token");
    vi.stubEnv("API_BASE_URL", "http://api:8080");
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.unstubAllEnvs();
  });

  it("renderiza filas y etiquetas españolas sin snapshots ni controles", async () => {
    const secret = "private-audit-snapshot-secret";
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(
        new Response(
          JSON.stringify({
            eventos: [{ ...baseEvent, estado_posterior: { authorization: secret } }],
          }),
          { status: 200 },
        ),
      ),
    );

    const markup = renderToStaticMarkup(await AuditPage());

    for (const label of ["Fecha", "Acción", "Actor", "Entidad", "Referencia", "Cambio"]) {
      expect(markup).toContain(label);
    }
    expect(markup).toContain("usuario.actualizado");
    expect(markup).toContain("Estado anterior y posterior registrados");
    expect(markup).not.toContain(secret);
    expect(markup).not.toContain("authorization");
    expect(markup).not.toContain("<button");
    expect(markup).not.toContain("Eliminar");
    expect(fetch).toHaveBeenCalledWith(
      expect.objectContaining({ pathname: "/internal/audit" }),
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

    const markup = renderToStaticMarkup(await AuditPage());
    expect(markup).toContain("No hay eventos de auditoría");
    expect(markup).toContain("No hay hechos de auditoría para mostrar.");
  });

  it("degrada sólo la página cuando falla el backend", async () => {
    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new Error("offline")));

    const markup = renderToStaticMarkup(await AuditPage());
    expect(markup).toContain("No se pudo consultar la auditoría");
    expect(markup).toContain('href="/internal"');
  });
});
