import { apiBaseUrl } from "@/lib/supabase/env";

export interface AuditDiagnostic {
  id: string;
  ocurrido_en: string;
  accion: string;
  entidad_tipo: string;
  entidad_id: string | null;
  referencia: string | null;
  actor_tipo: string;
  actor_usuario_id: string | null;
  tiene_estado_anterior: boolean;
  tiene_estado_posterior: boolean;
}

export interface AuditModel {
  status: "ready" | "unavailable";
  events: AuditDiagnostic[];
}

const AUDIT_REQUEST_TIMEOUT_MS = 5_000;

function nullableString(value: unknown): value is string | null {
  return value === null || typeof value === "string";
}

function isAuditDiagnostic(value: unknown): value is AuditDiagnostic {
  if (typeof value !== "object" || value === null) return false;
  const event = value as Record<string, unknown>;
  return (
    typeof event.id === "string" &&
    typeof event.ocurrido_en === "string" &&
    typeof event.accion === "string" &&
    typeof event.entidad_tipo === "string" &&
    nullableString(event.entidad_id) &&
    nullableString(event.referencia) &&
    typeof event.actor_tipo === "string" &&
    nullableString(event.actor_usuario_id) &&
    typeof event.tiene_estado_anterior === "boolean" &&
    typeof event.tiene_estado_posterior === "boolean"
  );
}

export async function loadAudit(accessToken: string | null): Promise<AuditModel> {
  const baseUrl = apiBaseUrl();
  if (!baseUrl) return { status: "unavailable", events: [] };

  try {
    const response = await fetch(new URL("/internal/audit?limite=50", baseUrl), {
      cache: "no-store",
      headers: accessToken ? { authorization: `Bearer ${accessToken}` } : undefined,
      signal: AbortSignal.timeout(AUDIT_REQUEST_TIMEOUT_MS),
    });
    if (!response.ok) return { status: "unavailable", events: [] };
    const payload: unknown = await response.json();
    if (typeof payload !== "object" || payload === null) {
      return { status: "unavailable", events: [] };
    }
    const events = (payload as Record<string, unknown>).eventos;
    if (!Array.isArray(events) || !events.every(isAuditDiagnostic)) {
      return { status: "unavailable", events: [] };
    }
    return { status: "ready", events };
  } catch {
    return { status: "unavailable", events: [] };
  }
}
