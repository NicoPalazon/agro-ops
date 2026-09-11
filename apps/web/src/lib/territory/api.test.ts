import { afterEach, describe, expect, it, vi } from "vitest";
import { loadTerritorialMapData } from "./api";

const geometry = {
  type: "MultiPolygon",
  coordinates: [
    [[[-60, -34], [-59.9, -34], [-59.9, -33.9], [-60, -34]]],
    [[[-59.5, -33.5], [-59.4, -33.5], [-59.4, -33.4], [-59.5, -33.5]]],
  ],
};

const establishment = {
  id: "e1",
  codigo: "CAMPO",
  nombre: "Campo",
  activo: true,
  geometria: geometry,
  referencias_externas: [],
  lotes_base: [],
};

const campaign = {
  id: "c1",
  codigo: "2026",
  nombre: "Campaña",
  fecha_inicio: "2026-01-01",
  fecha_fin: "2026-12-31",
  activa: true,
};

const unit = {
  id: "u1",
  campana_id: "c1",
  establecimiento_id: "e1",
  codigo: "UOP-A",
  nombre: "Unidad A",
  activa: true,
  geometria: geometry,
  lote_base_ids: [],
};

describe("territorial API client", () => {
  afterEach(() => {
    vi.unstubAllEnvs();
    vi.unstubAllGlobals();
  });

  it("loads authenticated API data while preserving the canonical MultiPolygon", async () => {
    vi.stubEnv("API_BASE_URL", "http://api:8080");
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: URL) => {
        switch (input.pathname) {
          case "/territorio/establecimientos":
            return new Response(JSON.stringify([establishment]));
          case "/territorio/campanas":
            return new Response(JSON.stringify([campaign]));
          case "/territorio/campanas/c1/establecimientos/e1/unidades-operativas":
            return new Response(JSON.stringify([unit]));
          case "/territorio/unidades-operativas/u1/usos":
            return new Response(JSON.stringify([]));
          default:
            return new Response(null, { status: 404 });
        }
      }),
    );

    const result = await loadTerritorialMapData("access-token");

    expect(result).toMatchObject({
      status: "ready",
      data: {
        establecimientos: [{ geometria: geometry }],
        unidades_operativas: [{ id: "u1", usos: [] }],
      },
    });
    expect(fetch).toHaveBeenCalledWith(
      expect.objectContaining({ pathname: "/territorio/establecimientos" }),
      expect.objectContaining({ headers: { authorization: "Bearer access-token" } }),
    );
  });

  it("does not fabricate a partial map model after a contextual API failure", async () => {
    vi.stubEnv("API_BASE_URL", "http://api:8080");
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: URL) => {
        if (input.pathname === "/territorio/establecimientos") {
          return new Response(JSON.stringify([establishment]));
        }
        if (input.pathname === "/territorio/campanas") {
          return new Response(JSON.stringify([campaign]));
        }
        return new Response(null, { status: 503 });
      }),
    );

    await expect(loadTerritorialMapData("access-token")).resolves.toEqual({
      status: "unavailable",
    });
  });
});
