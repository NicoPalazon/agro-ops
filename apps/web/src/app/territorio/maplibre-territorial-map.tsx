"use client";

import { useEffect, useRef, useState } from "react";
import type { GeoJSONSource, Map as MapLibreMap } from "maplibre-gl";
import type { TerritorialEntityReference, TerritorialMapLayers } from "./territorial-map-presentation";
import { selectTerritorialEntity, territorialBounds } from "./territorial-map-presentation";
import styles from "./territorio.module.css";

interface MapLibreTerritorialMapProps {
  layers: TerritorialMapLayers;
  selectedEntity: TerritorialEntityReference | null;
  onSelectEntity: (entity: TerritorialEntityReference | null) => void;
  drawingEnabled?: boolean;
  onDrawGeometry?: (geometry: { type: "Polygon"; coordinates: number[][][] }) => void;
}

const sourceIds = {
  establishments: "territory-establishments",
  lots: "territory-base-plots",
  units: "territory-operational-units",
} as const;

const layerIds = {
  establishmentFill: "territory-establishment-fill",
  establishmentLine: "territory-establishment-line",
  lotFill: "territory-base-plot-fill",
  lotLine: "territory-base-plot-line",
  unitFill: "territory-operational-unit-fill",
  unitLine: "territory-operational-unit-line",
} as const;

const clickableLayerIds = [
  layerIds.unitFill,
  layerIds.unitLine,
  layerIds.lotFill,
  layerIds.lotLine,
  layerIds.establishmentFill,
  layerIds.establishmentLine,
];

function updateSource(map: MapLibreMap, sourceId: string, data: unknown) {
  const source = map.getSource(sourceId) as GeoJSONSource | undefined;
  source?.setData(data as Parameters<GeoJSONSource["setData"]>[0]);
}

function installTerritoryLayers(map: MapLibreMap, layers: TerritorialMapLayers) {
  map.addSource(sourceIds.establishments, { type: "geojson", data: layers.establecimientos });
  map.addSource(sourceIds.lots, { type: "geojson", data: layers.lotesBase });
  map.addSource(sourceIds.units, { type: "geojson", data: layers.unidadesOperativas });

  map.addLayer({
    id: layerIds.establishmentFill,
    type: "fill",
    source: sourceIds.establishments,
    paint: { "fill-color": "#1d6f52", "fill-opacity": 0.13 },
  });
  map.addLayer({
    id: layerIds.establishmentLine,
    type: "line",
    source: sourceIds.establishments,
    paint: { "line-color": "#15533e", "line-width": 3 },
  });
  map.addLayer({
    id: layerIds.lotFill,
    type: "fill",
    source: sourceIds.lots,
    paint: { "fill-color": "#d1a33d", "fill-opacity": 0.24 },
  });
  map.addLayer({
    id: layerIds.lotLine,
    type: "line",
    source: sourceIds.lots,
    paint: { "line-color": "#8c6815", "line-width": 1.5, "line-dasharray": [2, 1] },
  });
  map.addLayer({
    id: layerIds.unitFill,
    type: "fill",
    source: sourceIds.units,
    paint: { "fill-color": "#315ca8", "fill-opacity": 0.3 },
  });
  map.addLayer({
    id: layerIds.unitLine,
    type: "line",
    source: sourceIds.units,
    paint: { "line-color": "#214782", "line-width": 2.5 },
  });
}

export function MapLibreTerritorialMap({
  layers,
  selectedEntity,
  onSelectEntity,
  drawingEnabled = false,
  onDrawGeometry,
}: MapLibreTerritorialMapProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const mapRef = useRef<MapLibreMap | null>(null);
  const layersInstalledRef = useRef(false);
  const onSelectRef = useRef(onSelectEntity);
  const drawingRef = useRef<number[][]>([]);
  const drawingEnabledRef = useRef(drawingEnabled);
  const onDrawRef = useRef(onDrawGeometry);
  const [ready, setReady] = useState(false);
  const [initializationError, setInitializationError] = useState(false);

  useEffect(() => {
    onSelectRef.current = onSelectEntity;
  }, [onSelectEntity]);

  useEffect(() => {
    drawingEnabledRef.current = drawingEnabled;
    onDrawRef.current = onDrawGeometry;
    if (!drawingEnabled) drawingRef.current = [];
  }, [drawingEnabled, onDrawGeometry]);

  useEffect(() => {
    let cancelled = false;
    let map: MapLibreMap | null = null;

    void import("maplibre-gl")
      .then(({ Map, NavigationControl }) => {
        if (cancelled || !containerRef.current) return;
        map = new Map({
          container: containerRef.current,
          center: [0, 0],
          zoom: 0,
          style: {
            version: 8,
            sources: {
              openstreetmap: {
                type: "raster",
                tiles: ["https://tile.openstreetmap.org/{z}/{x}/{y}.png"],
                tileSize: 256,
                attribution: "© OpenStreetMap contributors",
              },
            },
            layers: [{ id: "base", type: "raster", source: "openstreetmap" }],
          },
        });
        map.addControl(new NavigationControl(), "top-right");
        map.on("load", () => {
          if (!cancelled) setReady(true);
        });
        map.on("click", (event) => {
          if (drawingEnabledRef.current) {
            drawingRef.current = [...drawingRef.current, [event.lngLat.lng, event.lngLat.lat]];
            if (drawingRef.current.length >= 3) {
              const ring = [...drawingRef.current, drawingRef.current[0]];
              onDrawRef.current?.({ type: "Polygon", coordinates: [ring] });
            }
            return;
          }
          if (!layersInstalledRef.current) return;
          const features = map?.queryRenderedFeatures(event.point, {
            layers: clickableLayerIds,
          });
          const entity = selectTerritorialEntity(
            (features ?? []).map((item) => item.properties ?? {}),
          );
          if (entity) onSelectRef.current(entity);
        });
        map.on("mousemove", (event) => {
          if (!layersInstalledRef.current || !map) return;
          const hasFeature = map.queryRenderedFeatures(event.point, {
            layers: clickableLayerIds,
          }).length > 0;
          map.getCanvas().style.cursor = hasFeature ? "pointer" : "";
        });
        mapRef.current = map;
      })
      .catch(() => {
        if (!cancelled) setInitializationError(true);
      });

    return () => {
      cancelled = true;
      layersInstalledRef.current = false;
      map?.remove();
      if (mapRef.current === map) mapRef.current = null;
    };
  }, []);

  useEffect(() => {
    const map = mapRef.current;
    if (!map || !ready) return;

    if (!layersInstalledRef.current) {
      installTerritoryLayers(map, layers);
      layersInstalledRef.current = true;
    } else {
      updateSource(map, sourceIds.establishments, layers.establecimientos);
      updateSource(map, sourceIds.lots, layers.lotesBase);
      updateSource(map, sourceIds.units, layers.unidadesOperativas);
    }

    const bounds = territorialBounds(layers);
    if (bounds) map.fitBounds(bounds, { padding: 56, maxZoom: 15, duration: 0 });
  }, [layers, ready]);

  const selectedId = selectedEntity?.id ?? null;
  return (
    <div className={styles.mapFrame}>
      <div ref={containerRef} className={styles.map} aria-label="Mapa territorial" />
      {initializationError ? (
        <p className={styles.mapError} role="alert">
          No se pudo iniciar el mapa. La información territorial sigue disponible en la ficha.
        </p>
      ) : null}
      {selectedId ? <span className={styles.visuallyHidden}>Entidad seleccionada: {selectedId}</span> : null}
    </div>
  );
}
