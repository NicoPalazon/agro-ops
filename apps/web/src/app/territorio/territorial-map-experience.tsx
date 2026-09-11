"use client";

import { useMemo, useState } from "react";
import type { TerritorialMapData } from "@/lib/territory/api";
import { MapLibreTerritorialMap } from "./maplibre-territorial-map";
import {
  createTerritorialMapLayers,
  territorialFicha,
  type TerritorialEntityReference,
} from "./territorial-map-presentation";
import { TerritorialFichaPanel } from "./territorial-ficha";
import { TerritoryGeometryCapture } from "./territory-geometry-capture";
import styles from "./territorio.module.css";

interface TerritorialMapExperienceProps {
  data: TerritorialMapData;
}

export function TerritorialMapExperience({ data }: TerritorialMapExperienceProps) {
  const [selectedCampaignId, setSelectedCampaignId] = useState<string | null>(
    data.campanas.length === 1 ? data.campanas[0].id : null,
  );
  const [selectedEntity, setSelectedEntity] = useState<TerritorialEntityReference | null>(null);
  const [drawnGeometry, setDrawnGeometry] = useState<unknown>(null);
  const [drawingEnabled, setDrawingEnabled] = useState(false);

  const layers = useMemo(
    () => createTerritorialMapLayers(data, selectedCampaignId),
    [data, selectedCampaignId],
  );
  const ficha = useMemo(
    () => territorialFicha(data, selectedEntity),
    [data, selectedEntity],
  );

  const selectedCampaign = data.campanas.find((campaign) => campaign.id === selectedCampaignId);
  const isCampaignChoiceRequired = data.campanas.length > 1 && !selectedCampaignId;

  return (
    <main className={styles.page}>
      <header className={styles.header}>
        <div>
          <p className={styles.eyebrow}>Territorio</p>
          <h1>Mapa operativo</h1>
          <p>Establecimientos, lotes base y contexto de unidades operativas.</p>
        </div>
        <label className={styles.campaignControl}>
          <span>Contexto de campaña</span>
          <select
            aria-label="Contexto de campaña"
            value={selectedCampaignId ?? ""}
            onChange={(event) => {
              setSelectedCampaignId(event.target.value || null);
              setSelectedEntity(null);
            }}
          >
            {data.campanas.length !== 1 ? <option value="">Seleccionar campaña</option> : null}
            {data.campanas.map((campaign) => (
              <option key={campaign.id} value={campaign.id}>
                {campaign.codigo} · {campaign.nombre}
              </option>
            ))}
          </select>
        </label>
      </header>

      <section className={styles.workspace} aria-label="Experiencia territorial">
        <div className={styles.mapColumn}>
          <div className={styles.mapToolbar}>
            <p>
              {selectedCampaign
                ? `${selectedCampaign.codigo} · ${selectedCampaign.nombre}`
                : isCampaignChoiceRequired
                  ? "Elija una campaña para ver sus unidades operativas."
                  : "No hay campañas disponibles; se muestran los límites territoriales."}
            </p>
            <ul className={styles.legend} aria-label="Leyenda territorial">
              <li className={styles.legendEstablishment}>Establecimiento</li>
              <li className={styles.legendLot}>Lote base</li>
              <li className={styles.legendUnit}>Unidad operativa</li>
            </ul>
          </div>
          <MapLibreTerritorialMap
            layers={layers}
            selectedEntity={selectedEntity}
            onSelectEntity={setSelectedEntity}
            drawingEnabled={drawingEnabled}
            onDrawGeometry={setDrawnGeometry}
          />
        </div>
        <TerritoryGeometryCapture establecimientos={data.establecimientos} drawnGeometry={drawnGeometry} onDrawingModeChange={setDrawingEnabled} />
        <TerritorialFichaPanel ficha={ficha} />
      </section>
    </main>
  );
}
