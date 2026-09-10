import { apiBaseUrl } from "@/lib/supabase/env";
import type { JobState } from "../jobs/jobs";

export interface OutboxDiagnostic {
  id: string;
  destino: string;
  evento_tipo: string;
  entidad_tipo: string | null;
  entidad_id: string | null;
  referencia: string | null;
  idempotency_key: string;
  ocurrido_en: string;
  creado_en: string;
  estado: JobState;
  intentos: number;
  max_intentos: number;
  next_attempt_at: string;
  bloqueado_en: string | null;
  bloqueado_por: string | null;
  actualizado_en: string;
  completado_en: string | null;
  ultimo_error: string | null;
}

export interface OutboxModel {
  status: "ready" | "unavailable";
  events: OutboxDiagnostic[];
}

const OUTBOX_REQUEST_TIMEOUT_MS = 5_000;

function nullableString(value: unknown): value is string | null {
  return value === null || typeof value === "string";
}

function isJobState(value: unknown): value is JobState {
  return ["pendiente", "ejecutando", "completado", "agotado"].includes(
    value as string,
  );
}

function isOutboxDiagnostic(value: unknown): value is OutboxDiagnostic {
  if (typeof value !== "object" || value === null) return false;
  const event = value as Record<string, unknown>;
  return (
    typeof event.id === "string" &&
    typeof event.destino === "string" &&
    typeof event.evento_tipo === "string" &&
    nullableString(event.entidad_tipo) &&
    nullableString(event.entidad_id) &&
    nullableString(event.referencia) &&
    typeof event.idempotency_key === "string" &&
    typeof event.ocurrido_en === "string" &&
    typeof event.creado_en === "string" &&
    isJobState(event.estado) &&
    typeof event.intentos === "number" &&
    typeof event.max_intentos === "number" &&
    typeof event.next_attempt_at === "string" &&
    nullableString(event.bloqueado_en) &&
    nullableString(event.bloqueado_por) &&
    typeof event.actualizado_en === "string" &&
    nullableString(event.completado_en) &&
    nullableString(event.ultimo_error)
  );
}

export async function loadOutbox(
  accessToken: string | null,
): Promise<OutboxModel> {
  const baseUrl = apiBaseUrl();
  if (!baseUrl) return { status: "unavailable", events: [] };

  try {
    const response = await fetch(new URL("/internal/outbox?limite=50", baseUrl), {
      cache: "no-store",
      headers: accessToken ? { authorization: `Bearer ${accessToken}` } : undefined,
      signal: AbortSignal.timeout(OUTBOX_REQUEST_TIMEOUT_MS),
    });
    if (!response.ok) return { status: "unavailable", events: [] };
    const payload: unknown = await response.json();
    if (typeof payload !== "object" || payload === null) {
      return { status: "unavailable", events: [] };
    }
    const events = (payload as Record<string, unknown>).eventos;
    if (!Array.isArray(events) || !events.every(isOutboxDiagnostic)) {
      return { status: "unavailable", events: [] };
    }
    return { status: "ready", events };
  } catch {
    return { status: "unavailable", events: [] };
  }
}
