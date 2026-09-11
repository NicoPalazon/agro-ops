"use client";

import { useMemo, useRef, useState } from "react";
import type { Establecimiento } from "@/lib/territory/api";
import { createSupabaseBrowserClient } from "@/lib/supabase/browser";
import { apiBaseUrl } from "@/lib/supabase/env";
import styles from "./territorio.module.css";

export function TerritoryGeometryCapture({
  establecimientos,
  drawnGeometry,
  onDrawingModeChange,
}: {
  establecimientos: Establecimiento[];
  drawnGeometry: unknown;
  onDrawingModeChange: (enabled: boolean) => void;
}) {
  const [kind, setKind] = useState<"establecimiento" | "lote">("establecimiento");
  const [mode, setMode] = useState<"manual" | "importada">("importada");
  const [codigo, setCodigo] = useState("");
  const [nombre, setNombre] = useState("");
  const [renspa, setRenspa] = useState("");
  const [establecimientoId, setEstablecimientoId] = useState(establecimientos[0]?.id ?? "");
  const [geojsonText, setGeojsonText] = useState("");
  const [previewed, setPreviewed] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const idempotencyKeyRef = useRef<string | null>(null);
  const geometry = useMemo(() => {
    if (mode === "manual") return drawnGeometry;
    try { return JSON.parse(geojsonText) as unknown; } catch { return null; }
  }, [drawnGeometry, geojsonText, mode]);

  async function request(path: string, body: unknown, write = false) {
    const { data: { session } } = await createSupabaseBrowserClient().auth.getSession();
    const baseUrl = apiBaseUrl();
    if (!session?.access_token || !baseUrl) throw new Error("No se pudo validar la sesión.");
    const headers: Record<string, string> = { authorization: `Bearer ${session.access_token}`, "content-type": "application/json" };
    if (write) {
      idempotencyKeyRef.current ??= crypto.randomUUID();
      headers["idempotency-key"] = idempotencyKeyRef.current;
    }
    const response = await fetch(new URL(path, baseUrl), { method: "POST", headers, body: JSON.stringify(body) });
    const payload = await response.json().catch(() => null) as { mensaje?: string } | null;
    if (!response.ok) throw new Error(payload?.mensaje ?? "No se pudo completar la operación.");
  }

  async function preview() {
    setPreviewed(false); setError(null);
    if (!geometry) { setError("Indique un GeoJSON válido o dibuje al menos tres puntos en el mapa."); return; }
    try { await request("/territorio/previsualizaciones/geojson", { geometry }); setPreviewed(true); }
    catch (reason) { setError(reason instanceof Error ? reason.message : "No se pudo validar la geometría."); }
  }

  async function confirm() {
    if (!geometry || !previewed) return;
    setSaving(true); setError(null);
    try {
      const path = kind === "establecimiento" ? "/territorio/establecimientos" : `/territorio/establecimientos/${encodeURIComponent(establecimientoId)}/lotes-base`;
      const body = kind === "establecimiento" ? { codigo, nombre, tipo_origen: mode, geometry, renspa: renspa || null } : { codigo, nombre, geometry };
      await request(path, body, true);
      idempotencyKeyRef.current = null;
      window.location.reload();
    } catch (reason) { setError(reason instanceof Error ? reason.message : "No se pudo confirmar la geometría."); }
    finally { setSaving(false); }
  }

  return <aside className={styles.capturePanel} aria-label="Carga de geometría territorial">
    <p className={styles.fichaEyebrow}>Carga territorial</p>
    <h2>{kind === "establecimiento" ? "Nuevo establecimiento" : "Nuevo lote base"}</h2>
    <div className={styles.captureChoices}>
      <button type="button" className={kind === "establecimiento" ? styles.choiceActive : ""} onClick={() => { setKind("establecimiento"); setPreviewed(false); }}>Establecimiento</button>
      <button type="button" disabled={!establecimientos.length} className={kind === "lote" ? styles.choiceActive : ""} onClick={() => { setKind("lote"); setMode("manual"); onDrawingModeChange(true); setPreviewed(false); }}>Lote base</button>
    </div>
    {kind === "lote" ? <label>Establecimiento<select value={establecimientoId} onChange={(event) => setEstablecimientoId(event.target.value)}>{establecimientos.map((item) => <option key={item.id} value={item.id}>{item.codigo} · {item.nombre}</option>)}</select></label> : null}
    <label>Código<input value={codigo} onChange={(event) => { setCodigo(event.target.value); setPreviewed(false); }} placeholder="CAMPO_NORTE" /></label>
    <label>Nombre<input value={nombre} onChange={(event) => { setNombre(event.target.value); setPreviewed(false); }} placeholder="Campo Norte" /></label>
    {kind === "establecimiento" ? <><div className={styles.captureChoices}><button type="button" className={mode === "manual" ? styles.choiceActive : ""} onClick={() => { setMode("manual"); onDrawingModeChange(true); setPreviewed(false); }}>Dibujar en mapa</button><button type="button" className={mode === "importada" ? styles.choiceActive : ""} onClick={() => { setMode("importada"); onDrawingModeChange(false); setPreviewed(false); }}>Importar GeoJSON</button></div><label>RENSPA (opcional)<input value={renspa} onChange={(event) => setRenspa(event.target.value)} placeholder="Referencia SENASA" /></label></> : null}
    {kind === "lote" ? <div className={styles.captureChoices}><button type="button" className={mode === "manual" ? styles.choiceActive : ""} onClick={() => { setMode("manual"); onDrawingModeChange(true); setPreviewed(false); }}>Dibujar en mapa</button><button type="button" className={mode === "importada" ? styles.choiceActive : ""} onClick={() => { setMode("importada"); onDrawingModeChange(false); setPreviewed(false); }}>Importar GeoJSON</button></div> : null}
    {mode === "manual" ? <p className={styles.captureHint}>Haga clic en al menos tres puntos del mapa. El polígono se cierra automáticamente.</p> : <label>GeoJSON<textarea rows={7} value={geojsonText} onChange={(event) => { setGeojsonText(event.target.value); setPreviewed(false); }} placeholder='{"type":"Polygon","coordinates":[...]}' /></label>}
    <button type="button" className={styles.primaryButton} onClick={() => void preview()}>Previsualizar geometría</button>
    {previewed ? <p className={styles.validState}>Geometría válida · MultiPolygon · SRID 4326</p> : null}
    {error ? <p role="alert" className={styles.validationState}>{error}</p> : null}
    <button type="button" className={styles.primaryButton} disabled={!previewed || saving || !codigo || !nombre || (kind === "lote" && !establecimientoId)} onClick={() => void confirm()}>{saving ? "Confirmando…" : "Confirmar creación"}</button>
    <p className={styles.captureHint}>Una imagen o plano puede servir de referencia visual, pero la geometría válida es siempre la confirmada por PostGIS.</p>
  </aside>;
}
