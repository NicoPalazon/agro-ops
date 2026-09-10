import { apiBaseUrl } from "@/lib/supabase/env";

export interface IdempotencyDiagnostic {
  id: string;
  operacion: string;
  idempotency_key: string;
  request_sha256: string;
  completado_en: string;
  resultado_bytes: number;
}

export interface IdempotencyModel {
  status: "ready" | "unavailable";
  records: IdempotencyDiagnostic[];
}

const IDEMPOTENCY_REQUEST_TIMEOUT_MS = 5_000;

function isIdempotencyDiagnostic(value: unknown): value is IdempotencyDiagnostic {
  if (typeof value !== "object" || value === null) return false;
  const record = value as Record<string, unknown>;
  return (
    typeof record.id === "string" &&
    typeof record.operacion === "string" &&
    typeof record.idempotency_key === "string" &&
    typeof record.request_sha256 === "string" &&
    typeof record.completado_en === "string" &&
    typeof record.resultado_bytes === "number"
  );
}

export async function loadIdempotency(
  accessToken: string | null,
): Promise<IdempotencyModel> {
  const baseUrl = apiBaseUrl();
  if (!baseUrl) return { status: "unavailable", records: [] };

  try {
    const response = await fetch(
      new URL("/internal/idempotencia?limite=50", baseUrl),
      {
        cache: "no-store",
        headers: accessToken
          ? { authorization: `Bearer ${accessToken}` }
          : undefined,
        signal: AbortSignal.timeout(IDEMPOTENCY_REQUEST_TIMEOUT_MS),
      },
    );
    if (!response.ok) return { status: "unavailable", records: [] };
    const payload: unknown = await response.json();
    if (typeof payload !== "object" || payload === null) {
      return { status: "unavailable", records: [] };
    }
    const records = (payload as Record<string, unknown>).registros;
    if (!Array.isArray(records) || !records.every(isIdempotencyDiagnostic)) {
      return { status: "unavailable", records: [] };
    }
    return { status: "ready", records };
  } catch {
    return { status: "unavailable", records: [] };
  }
}
