import { describe, expect, it } from "vitest";
import type { TerritorialGeometry, TerritorialMapData } from "@/lib/territory/api";
import {
  createTerritorialMapLayers,
  selectTerritorialEntity,
  territorialBounds,
  territorialFicha,
} from "./territorial-map-presentation";

const disconnectedGeometry: TerritorialGeometry = {
  type: "MultiPolygon" as const,
  coordinates: [
    [[[-60, -34], [-59.9, -34], [-59.9, -33.9], [-60, -34]]],
    [[[-59.5, -33.5], [-59.4, -33.5], [-59.4, -33.4], [-59.5, -33.5]]],
  ],
};

const mapData: TerritorialMapData = {
  establecimientos: [
    {
      id: "establishment-1",
      codigo: "CAMPO",
      nombre: "Campo Norte",
      activo: true,
      geometria: disconnectedGeometry,
      referencias_externas: [{ sistema_externo: "SENASA", external_id: "REF-1" }],
      lotes_base: [
        {
          id: "lot-1",
          codigo: "A",
          nombre: "Lote A",
          activo: true,
          geometria: {
            type: "MultiPolygon",
            coordinates: [[[[-60, -34], [-59.9, -34], [-59.9, -33.9], [-60, -34]]]],
          } as TerritorialGeometry,
        },
      ],
    },
  ],
  campanas: [
    {
      id: "campaign-1",
      codigo: "2026A",
      nombre: "Primera",
      fecha_inicio: "2026-01-01",
      fecha_fin: "2026-06-30",
      activa: true,
    },
    {
      id: "campaign-2",
      codigo: "2026B",
      nombre: "Superpuesta",
      fecha_inicio: "2026-05-01",
      fecha_fin: "2026-12-31",
      activa: true,
    },
  ],
  unidades_operativas: [
    {
      id: "unit-1",
      campana_id: "campaign-1",
      establecimiento_id: "establishment-1",
      codigo: "UOP-A",
      nombre: "Unidad A",
      activa: true,
      geometria: disconnectedGeometry,
      lote_base_ids: ["lot-1"],
      usos: [
        {
          id: "use-1",
          uso_territorial_id: "territorial-use-1",
          uso_codigo: "PASTURA",
          uso_nombre: "Pastura",
          fecha_inicio: "2026-01-01",
          fecha_fin: "2026-03-31",
        },
      ],
    },
    {
      id: "unit-2",
      campana_id: "campaign-2",
      establecimiento_id: "establishment-1",
      codigo: "UOP-B",
      nombre: "Unidad B",
      activa: true,
      geometria: disconnectedGeometry,
      lote_base_ids: ["lot-1"],
      usos: [],
    },
  ],
};

describe("territorial map presentation", () => {
  it("keeps a disconnected MultiPolygon as one establishment feature", () => {
    const layers = createTerritorialMapLayers(mapData, null);

    expect(layers.establecimientos.features).toHaveLength(1);
    expect(layers.establecimientos.features[0].geometry).toEqual(disconnectedGeometry);
    expect(layers.establecimientos.features[0].geometry.coordinates).toHaveLength(2);
    expect(territorialBounds(layers)).toEqual([[-60, -34], [-59.4, -33.4]]);
  });

  it("only presents UOPs from the explicitly selected campaign", () => {
    expect(createTerritorialMapLayers(mapData, null).unidadesOperativas.features).toHaveLength(0);
    expect(createTerritorialMapLayers(mapData, "campaign-1").unidadesOperativas.features).toMatchObject([
      { properties: { entity_id: "unit-1", campana_id: "campaign-1" } },
    ]);
    expect(createTerritorialMapLayers(mapData, "campaign-2").unidadesOperativas.features).toMatchObject([
      { properties: { entity_id: "unit-2", campana_id: "campaign-2" } },
    ]);
  });

  it("uses UOP then LoteBase then Establecimiento as deterministic click priority", () => {
    expect(
      selectTerritorialEntity([
        { entity_type: "establecimiento", entity_id: "establishment-1" },
        { entity_type: "lote_base", entity_id: "lot-1" },
        { entity_type: "unidad_operativa", entity_id: "unit-1" },
      ]),
    ).toEqual({ type: "unidad_operativa", id: "unit-1" });
    expect(
      selectTerritorialEntity([
        { entity_type: "establecimiento", entity_id: "establishment-1" },
        { entity_type: "lote_base", entity_id: "lot-1" },
      ]),
    ).toEqual({ type: "lote_base", id: "lot-1" });
  });

  it("builds a contextual UOP ficha without deriving territorial rules", () => {
    expect(territorialFicha(mapData, { type: "unidad_operativa", id: "unit-1" })).toMatchObject({
      codigo: "UOP-A",
      establecimiento: { codigo: "CAMPO" },
      campana: { codigo: "2026A" },
      usos: [{ uso_codigo: "PASTURA" }],
      loteBaseIds: ["lot-1"],
    });
  });
});
