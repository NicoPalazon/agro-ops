import type {
  Campana,
  Establecimiento,
  Position,
  TerritorialGeometry,
  TerritorialMapData,
  UnidadOperativaConUsos,
} from "@/lib/territory/api";

export type TerritorialEntityType = "establecimiento" | "lote_base" | "unidad_operativa";

export interface TerritorialEntityReference {
  type: TerritorialEntityType;
  id: string;
}

interface MapFeatureProperties {
  entity_type: TerritorialEntityType;
  entity_id: string;
  establecimiento_id?: string;
  campana_id?: string;
}

export interface TerritorialFeature {
  type: "Feature";
  properties: MapFeatureProperties;
  geometry: TerritorialGeometry;
}

export interface TerritorialFeatureCollection {
  type: "FeatureCollection";
  features: TerritorialFeature[];
}

export interface TerritorialMapLayers {
  establecimientos: TerritorialFeatureCollection;
  lotesBase: TerritorialFeatureCollection;
  unidadesOperativas: TerritorialFeatureCollection;
}

export interface TerritorialFicha {
  entityType: TerritorialEntityType;
  codigo: string;
  nombre: string;
  establecimiento?: Pick<Establecimiento, "codigo" | "nombre">;
  campana?: Pick<Campana, "codigo" | "nombre" | "fecha_inicio" | "fecha_fin">;
  usos?: UnidadOperativaConUsos["usos"];
  referenciasExternas?: Establecimiento["referencias_externas"];
  loteBaseIds?: string[];
}

function collection(features: TerritorialFeature[]): TerritorialFeatureCollection {
  return { type: "FeatureCollection", features };
}

function feature(
  type: TerritorialEntityType,
  id: string,
  geometry: TerritorialGeometry,
  context: Omit<MapFeatureProperties, "entity_type" | "entity_id"> = {},
): TerritorialFeature {
  return {
    type: "Feature",
    properties: { entity_type: type, entity_id: id, ...context },
    // Geometry is passed through as returned by the territorial read API. In
    // particular, no MultiPolygon component is split, joined, or filled here.
    geometry,
  };
}

export function createTerritorialMapLayers(
  data: TerritorialMapData,
  selectedCampaignId: string | null,
): TerritorialMapLayers {
  return {
    establecimientos: collection(
      data.establecimientos.map((establishment) =>
        feature("establecimiento", establishment.id, establishment.geometria),
      ),
    ),
    lotesBase: collection(
      data.establecimientos.flatMap((establishment) =>
        establishment.lotes_base.map((lote) =>
          feature("lote_base", lote.id, lote.geometria, {
            establecimiento_id: establishment.id,
          }),
        ),
      ),
    ),
    unidadesOperativas: collection(
      selectedCampaignId
        ? data.unidades_operativas
            .filter((unit) => unit.campana_id === selectedCampaignId)
            .map((unit) =>
              feature("unidad_operativa", unit.id, unit.geometria, {
                establecimiento_id: unit.establecimiento_id,
                campana_id: unit.campana_id,
              }),
            )
        : [],
    ),
  };
}

const entityPriority: Record<TerritorialEntityType, number> = {
  establecimiento: 1,
  lote_base: 2,
  unidad_operativa: 3,
};

function isEntityType(value: unknown): value is TerritorialEntityType {
  return value === "establecimiento" || value === "lote_base" || value === "unidad_operativa";
}

/** The deterministic selection order is UOP, LoteBase, then Establecimiento. */
export function selectTerritorialEntity(
  candidates: Array<{ entity_type?: unknown; entity_id?: unknown }>,
): TerritorialEntityReference | null {
  let selected: TerritorialEntityReference | null = null;

  for (const candidate of candidates) {
    if (!isEntityType(candidate.entity_type) || typeof candidate.entity_id !== "string") {
      continue;
    }
    if (!selected || entityPriority[candidate.entity_type] > entityPriority[selected.type]) {
      selected = { type: candidate.entity_type, id: candidate.entity_id };
    }
  }

  return selected;
}

export function territorialFicha(
  data: TerritorialMapData,
  entity: TerritorialEntityReference | null,
): TerritorialFicha | null {
  if (!entity) return null;

  if (entity.type === "establecimiento") {
    const establishment = data.establecimientos.find((item) => item.id === entity.id);
    return establishment
      ? {
          entityType: entity.type,
          codigo: establishment.codigo,
          nombre: establishment.nombre,
          referenciasExternas: establishment.referencias_externas,
        }
      : null;
  }

  if (entity.type === "lote_base") {
    const establishment = data.establecimientos.find((item) =>
      item.lotes_base.some((lote) => lote.id === entity.id),
    );
    const lote = establishment?.lotes_base.find((item) => item.id === entity.id);
    return establishment && lote
      ? {
          entityType: entity.type,
          codigo: lote.codigo,
          nombre: lote.nombre,
          establecimiento: { codigo: establishment.codigo, nombre: establishment.nombre },
        }
      : null;
  }

  const unit = data.unidades_operativas.find((item) => item.id === entity.id);
  const establishment = unit
    ? data.establecimientos.find((item) => item.id === unit.establecimiento_id)
    : undefined;
  const campaign = unit ? data.campanas.find((item) => item.id === unit.campana_id) : undefined;
  return unit
    ? {
        entityType: entity.type,
        codigo: unit.codigo,
        nombre: unit.nombre,
        establecimiento: establishment
          ? { codigo: establishment.codigo, nombre: establishment.nombre }
          : undefined,
        campana: campaign
          ? {
              codigo: campaign.codigo,
              nombre: campaign.nombre,
              fecha_inicio: campaign.fecha_inicio,
              fecha_fin: campaign.fecha_fin,
            }
          : undefined,
        usos: unit.usos,
        loteBaseIds: unit.lote_base_ids,
      }
    : null;
}

export type TerritorialBounds = [[number, number], [number, number]];

/** Calculates display bounds only; it never becomes territorial business data. */
export function territorialBounds(layers: TerritorialMapLayers): TerritorialBounds | null {
  const positions: Position[] = [
    layers.establecimientos,
    layers.lotesBase,
    layers.unidadesOperativas,
  ].flatMap((layer) =>
    layer.features.flatMap((item) =>
      item.geometry.coordinates.flatMap((polygon) => polygon.flatMap((ring) => ring)),
    ),
  );
  if (positions.length === 0) return null;

  const longitudes = positions.map(([longitude]) => longitude);
  const latitudes = positions.map(([, latitude]) => latitude);
  return [
    [Math.min(...longitudes), Math.min(...latitudes)],
    [Math.max(...longitudes), Math.max(...latitudes)],
  ];
}
