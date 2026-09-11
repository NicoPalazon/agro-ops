import { apiBaseUrl } from "@/lib/supabase/env";

export type Position = [number, number];
export type MultiPolygonCoordinates = Position[][][];

export interface TerritorialGeometry {
  type: "MultiPolygon";
  coordinates: MultiPolygonCoordinates;
}

export interface LoteBase {
  id: string;
  codigo: string;
  nombre: string;
  activo: boolean;
  geometria: TerritorialGeometry;
}

export interface ExternalReference {
  sistema_externo: string;
  external_id: string;
}

export interface Establecimiento {
  id: string;
  codigo: string;
  nombre: string;
  activo: boolean;
  geometria: TerritorialGeometry;
  referencias_externas: ExternalReference[];
  lotes_base: LoteBase[];
}

export interface Campana {
  id: string;
  codigo: string;
  nombre: string;
  fecha_inicio: string;
  fecha_fin: string;
  activa: boolean;
}

export interface UsoTerritorial {
  id: string;
  uso_territorial_id: string;
  uso_codigo: string;
  uso_nombre: string;
  fecha_inicio: string;
  fecha_fin: string;
}

export interface UnidadOperativa {
  id: string;
  campana_id: string;
  establecimiento_id: string;
  codigo: string;
  nombre: string;
  activa: boolean;
  geometria: TerritorialGeometry;
  lote_base_ids: string[];
}

export interface UnidadOperativaConUsos extends UnidadOperativa {
  usos: UsoTerritorial[];
}

export interface TerritorialMapData {
  establecimientos: Establecimiento[];
  campanas: Campana[];
  unidades_operativas: UnidadOperativaConUsos[];
}

export type TerritorialMapLoadResult =
  | { status: "ready"; data: TerritorialMapData }
  | { status: "unavailable" };

const TERRITORY_REQUEST_TIMEOUT_MS = 5_000;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isPosition(value: unknown): value is Position {
  return (
    Array.isArray(value) &&
    value.length >= 2 &&
    typeof value[0] === "number" &&
    Number.isFinite(value[0]) &&
    typeof value[1] === "number" &&
    Number.isFinite(value[1])
  );
}

function isMultiPolygonCoordinates(value: unknown): value is MultiPolygonCoordinates {
  return (
    Array.isArray(value) &&
    value.every(
      (polygon) =>
        Array.isArray(polygon) &&
        polygon.every((ring) => Array.isArray(ring) && ring.every(isPosition)),
    )
  );
}

function isGeometry(value: unknown): value is TerritorialGeometry {
  return (
    isRecord(value) &&
    value.type === "MultiPolygon" &&
    isMultiPolygonCoordinates(value.coordinates)
  );
}

function isLoteBase(value: unknown): value is LoteBase {
  return (
    isRecord(value) &&
    typeof value.id === "string" &&
    typeof value.codigo === "string" &&
    typeof value.nombre === "string" &&
    typeof value.activo === "boolean" &&
    isGeometry(value.geometria)
  );
}

function isExternalReference(value: unknown): value is ExternalReference {
  return (
    isRecord(value) &&
    typeof value.sistema_externo === "string" &&
    typeof value.external_id === "string"
  );
}

function isEstablecimiento(value: unknown): value is Establecimiento {
  return (
    isRecord(value) &&
    typeof value.id === "string" &&
    typeof value.codigo === "string" &&
    typeof value.nombre === "string" &&
    typeof value.activo === "boolean" &&
    isGeometry(value.geometria) &&
    Array.isArray(value.referencias_externas) &&
    value.referencias_externas.every(isExternalReference) &&
    Array.isArray(value.lotes_base) &&
    value.lotes_base.every(isLoteBase)
  );
}

function isCampana(value: unknown): value is Campana {
  return (
    isRecord(value) &&
    typeof value.id === "string" &&
    typeof value.codigo === "string" &&
    typeof value.nombre === "string" &&
    typeof value.fecha_inicio === "string" &&
    typeof value.fecha_fin === "string" &&
    typeof value.activa === "boolean"
  );
}

function isUsoTerritorial(value: unknown): value is UsoTerritorial {
  return (
    isRecord(value) &&
    typeof value.id === "string" &&
    typeof value.uso_territorial_id === "string" &&
    typeof value.uso_codigo === "string" &&
    typeof value.uso_nombre === "string" &&
    typeof value.fecha_inicio === "string" &&
    typeof value.fecha_fin === "string"
  );
}

function isUnidadOperativa(value: unknown): value is UnidadOperativa {
  return (
    isRecord(value) &&
    typeof value.id === "string" &&
    typeof value.campana_id === "string" &&
    typeof value.establecimiento_id === "string" &&
    typeof value.codigo === "string" &&
    typeof value.nombre === "string" &&
    typeof value.activa === "boolean" &&
    isGeometry(value.geometria) &&
    Array.isArray(value.lote_base_ids) &&
    value.lote_base_ids.every((id) => typeof id === "string")
  );
}

async function requestJson(
  path: string,
  accessToken: string | null,
): Promise<unknown | null> {
  try {
    const baseUrl = apiBaseUrl();
    if (!baseUrl) return null;
    const response = await fetch(new URL(path, baseUrl), {
      cache: "no-store",
      headers: accessToken ? { authorization: `Bearer ${accessToken}` } : undefined,
      signal: AbortSignal.timeout(TERRITORY_REQUEST_TIMEOUT_MS),
    });
    return response.ok ? await response.json() : null;
  } catch {
    return null;
  }
}

async function requestList<T>(
  path: string,
  accessToken: string | null,
  isItem: (value: unknown) => value is T,
): Promise<T[] | null> {
  const payload = await requestJson(path, accessToken);
  return Array.isArray(payload) && payload.every(isItem) ? payload : null;
}

/**
 * Loads the complete read model needed by the map. A failed contextual request
 * makes the model unavailable rather than presenting a partial territorial view.
 */
export async function loadTerritorialMapData(
  accessToken: string | null,
): Promise<TerritorialMapLoadResult> {
  let establishments: Establecimiento[] | null;
  let campaigns: Campana[] | null;

  try {
    [establishments, campaigns] = await Promise.all([
      requestList("/territorio/establecimientos", accessToken, isEstablecimiento),
      requestList("/territorio/campanas", accessToken, isCampana),
    ]);
  } catch {
    return { status: "unavailable" };
  }

  if (!establishments || !campaigns) return { status: "unavailable" };

  const unitContexts = await Promise.all(
    campaigns.flatMap((campaign) =>
      establishments.map(async (establishment) => {
        const units = await requestList(
          `/territorio/campanas/${encodeURIComponent(campaign.id)}/establecimientos/${encodeURIComponent(establishment.id)}/unidades-operativas`,
          accessToken,
          isUnidadOperativa,
        );
        return units;
      }),
    ),
  );
  if (unitContexts.some((units) => units === null)) return { status: "unavailable" };

  const units = unitContexts.flatMap((context) => context ?? []);
  const unitsWithUses = await Promise.all(
    units.map(async (unit) => {
      const uses = await requestList(
        `/territorio/unidades-operativas/${encodeURIComponent(unit.id)}/usos`,
        accessToken,
        isUsoTerritorial,
      );
      return uses === null ? null : { ...unit, usos: uses };
    }),
  );
  if (unitsWithUses.some((unit) => unit === null)) return { status: "unavailable" };

  return {
    status: "ready",
    data: {
      establecimientos: establishments,
      campanas: campaigns,
      unidades_operativas: unitsWithUses.filter(
        (unit): unit is UnidadOperativaConUsos => unit !== null,
      ),
    },
  };
}
