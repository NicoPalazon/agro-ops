import { apiBaseUrl } from "@/lib/supabase/env";

export type JobState = "pendiente" | "ejecutando" | "completado" | "agotado";

export interface JobDiagnostic {
  id: string;
  tipo: string;
  estado: JobState;
  intentos: number;
  max_intentos: number;
  next_attempt_at: string;
  bloqueado_en: string | null;
  bloqueado_por: string | null;
  creado_en: string;
  actualizado_en: string;
  completado_en: string | null;
  ultimo_error: string | null;
}

export interface JobsModel {
  status: "ready" | "unavailable";
  jobs: JobDiagnostic[];
}

export const JOBS_REQUEST_TIMEOUT_MS = 5_000;

function isNullableString(value: unknown): value is string | null {
  return value === null || typeof value === "string";
}

function isJobState(value: unknown): value is JobState {
  return ["pendiente", "ejecutando", "completado", "agotado"].includes(
    value as string,
  );
}

function isJobDiagnostic(value: unknown): value is JobDiagnostic {
  if (typeof value !== "object" || value === null) return false;
  const job = value as Record<string, unknown>;
  return (
    typeof job.id === "string" &&
    typeof job.tipo === "string" &&
    isJobState(job.estado) &&
    typeof job.intentos === "number" &&
    typeof job.max_intentos === "number" &&
    typeof job.next_attempt_at === "string" &&
    isNullableString(job.bloqueado_en) &&
    isNullableString(job.bloqueado_por) &&
    typeof job.creado_en === "string" &&
    typeof job.actualizado_en === "string" &&
    isNullableString(job.completado_en) &&
    isNullableString(job.ultimo_error)
  );
}

export async function loadJobs(accessToken: string | null): Promise<JobsModel> {
  const baseUrl = apiBaseUrl();
  if (!baseUrl) return { status: "unavailable", jobs: [] };

  try {
    const response = await fetch(new URL("/internal/jobs?limite=50", baseUrl), {
      cache: "no-store",
      headers: accessToken ? { authorization: `Bearer ${accessToken}` } : undefined,
      signal: AbortSignal.timeout(JOBS_REQUEST_TIMEOUT_MS),
    });
    if (!response.ok) return { status: "unavailable", jobs: [] };
    const payload: unknown = await response.json();
    if (typeof payload !== "object" || payload === null) {
      return { status: "unavailable", jobs: [] };
    }
    const jobs = (payload as Record<string, unknown>).jobs;
    if (!Array.isArray(jobs) || !jobs.every(isJobDiagnostic)) {
      return { status: "unavailable", jobs: [] };
    }
    return { status: "ready", jobs };
  } catch {
    return { status: "unavailable", jobs: [] };
  }
}
