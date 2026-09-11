import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { TerritorialGeometry, TerritorialMapData } from "@/lib/territory/api";
import { TerritorialMapExperience } from "./territorial-map-experience";

const geometry: TerritorialGeometry = {
  type: "MultiPolygon" as const,
  coordinates: [[[[0, 0], [1, 0], [1, 1], [0, 0]]]],
};

function data(campaignCount: number): TerritorialMapData {
  return {
    establecimientos: [
      {
        id: "e1",
        codigo: "CAMPO",
        nombre: "Campo",
        activo: true,
        geometria: geometry,
        referencias_externas: [],
        lotes_base: [],
      },
    ],
    campanas: Array.from({ length: campaignCount }, (_, index) => ({
      id: `c${index + 1}`,
      codigo: `C${index + 1}`,
      nombre: `Campaña ${index + 1}`,
      fecha_inicio: "2026-01-01",
      fecha_fin: "2026-12-31",
      activa: true,
    })),
    unidades_operativas: [],
  };
}

describe("TerritorialMapExperience", () => {
  it("requires an explicit campaign choice when the API returns overlapping contexts", () => {
    const markup = renderToStaticMarkup(<TerritorialMapExperience data={data(2)} />);

    expect(markup).toContain("Seleccionar campaña");
    expect(markup).toContain("Elija una campaña para ver sus unidades operativas.");
    expect(markup).not.toContain('option value="c1" selected=""');
  });

  it("shows a clear empty state when no establishments are visible", () => {
    const markup = renderToStaticMarkup(
      <TerritorialMapExperience data={{ ...data(0), establecimientos: [] }} />,
    );

    expect(markup).toContain("No hay información territorial");
    expect(markup).not.toContain("Mapa operativo");
  });
});
