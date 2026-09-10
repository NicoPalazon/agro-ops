import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const auth = vi.hoisted(() => ({ accessToken: vi.fn() }));

vi.mock("@/lib/auth/server", () => ({
  authenticatedAccessToken: auth.accessToken,
}));

import JobsPage from "./page";

const baseJob = {
  id: "01991d89-6a00-7000-8000-000000000001",
  tipo: "test.procesar",
  intentos: 1,
  max_intentos: 3,
  next_attempt_at: "2026-09-09T15:00:00.000000Z",
  bloqueado_en: null,
  bloqueado_por: null,
  creado_en: "2026-09-09T14:00:00.000000Z",
  actualizado_en: "2026-09-09T14:30:00.000000Z",
  completado_en: null,
  ultimo_error: null,
};

describe("Diagnóstico de jobs", () => {
  beforeEach(() => {
    auth.accessToken.mockResolvedValue("authenticated-access-token");
    vi.stubEnv("API_BASE_URL", "http://api:8080");
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.unstubAllEnvs();
  });

  it("renderiza metadatos seguros y las etiquetas españolas de cada estado", async () => {
    const secret = "private-payload-secret";
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(
        new Response(
          JSON.stringify({
            jobs: ["pendiente", "ejecutando", "completado", "agotado"].map(
              (estado, index) => ({
                ...baseJob,
                id: `${baseJob.id.slice(0, -1)}${index + 1}`,
                estado,
                payload: { authorization: secret },
              }),
            ),
          }),
          { status: 200 },
        ),
      ),
    );

    const markup = renderToStaticMarkup(await JobsPage());

    for (const label of ["Pendiente", "Ejecutando", "Completado", "Agotado"]) {
      expect(markup).toContain(label);
    }
    expect(markup).toContain("test.procesar");
    expect(markup).toContain("1 / 3");
    expect(markup).not.toContain(secret);
    expect(markup).not.toContain("authorization");
    expect(fetch).toHaveBeenCalledWith(
      expect.objectContaining({ pathname: "/internal/jobs" }),
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
        new Response(JSON.stringify({ jobs: [] }), { status: 200 }),
      ),
    );

    const markup = renderToStaticMarkup(await JobsPage());
    expect(markup).toContain("La cola está vacía");
    expect(markup).toContain("No hay jobs persistidos para mostrar.");
  });

  it("degrada sólo la página cuando falla el backend", async () => {
    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new Error("offline")));

    const markup = renderToStaticMarkup(await JobsPage());
    expect(markup).toContain("No se pudo consultar la cola");
    expect(markup).toContain(
      "El diagnóstico de jobs no está disponible temporalmente.",
    );
    expect(markup).toContain('href="/internal"');
  });

  it("rechaza respuestas que no respetan el contrato seguro", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(
        new Response(JSON.stringify({ jobs: [{ tipo: "test" }] }), {
          status: 200,
        }),
      ),
    );

    const markup = renderToStaticMarkup(await JobsPage());
    expect(markup).toContain("No se pudo consultar la cola");
  });
});
