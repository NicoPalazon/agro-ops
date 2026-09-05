import { apiBaseUrl } from "@/lib/supabase/env";

export type DisplayStatus =
  | "operational"
  | "degraded"
  | "stale"
  | "unavailable";

export interface ServiceCheck {
  service: "web" | "api" | "database" | "worker";
  label: string;
  status: DisplayStatus;
  detail: string;
}

export interface SystemStatusModel {
  services: ServiceCheck[];
  backendVersion: string | null;
}

interface FetchResult {
  outcome: "success" | "failed" | "unavailable";
  data?: unknown;
}

const unavailableDetail = "The check could not reach the backend.";
export const BACKEND_REQUEST_TIMEOUT_MS = 5_000;

async function fetchInternal(
  path: string,
  accessToken?: string | null,
): Promise<FetchResult> {
  const baseUrl = apiBaseUrl();

  if (!baseUrl) {
    return { outcome: "unavailable" };
  }

  try {
    const response = await fetch(new URL(path, baseUrl), {
      cache: "no-store",
      headers: accessToken ? { authorization: `Bearer ${accessToken}` } : undefined,
      signal: AbortSignal.timeout(BACKEND_REQUEST_TIMEOUT_MS),
    });

    if (!response.ok) {
      return { outcome: "failed" };
    }

    return {
      outcome: "success",
      data: await response.json(),
    };
  } catch {
    return { outcome: "unavailable" };
  }
}

function record(value: unknown): Record<string, unknown> | null {
  return typeof value === "object" && value !== null
    ? (value as Record<string, unknown>)
    : null;
}

function simpleServiceCheck(
  service: "api" | "database",
  label: string,
  result: FetchResult,
  payload: unknown,
  expectedStatus: string,
): ServiceCheck {
  if (result.outcome === "unavailable") {
    return { service, label, status: "unavailable", detail: unavailableDetail };
  }

  const servicePayload = record(payload);

  if (result.outcome === "success" && servicePayload?.status === expectedStatus) {
    return {
      service,
      label,
      status: "operational",
      detail: `${label} check completed successfully.`,
    };
  }

  return {
    service,
    label,
    status: "degraded",
    detail: `${label} check did not report a healthy state.`,
  };
}

function workerServiceCheck(result: FetchResult, payload: unknown): ServiceCheck {
  if (result.outcome === "unavailable") {
    return {
      service: "worker",
      label: "Worker",
      status: "unavailable",
      detail: unavailableDetail,
    };
  }

  const workerPayload = record(payload);
  const status = workerPayload?.status;
  const lastSeenAt =
    typeof workerPayload?.last_seen_at === "string"
      ? workerPayload.last_seen_at
      : null;

  if (
    result.outcome === "success" &&
    (status === "healthy" || status === "stale" || status === "unavailable")
  ) {
    return {
      service: "worker",
      label: "Worker",
      status: status === "healthy" ? "operational" : status,
      detail: lastSeenAt
        ? `Last heartbeat: ${lastSeenAt}`
        : "No persisted worker heartbeat is available.",
    };
  }

  return {
    service: "worker",
    label: "Worker",
    status: "degraded",
    detail: "The worker check returned an invalid response.",
  };
}

function backendVersion(result: FetchResult, payload: unknown): string | null {
  const versionPayload = record(payload);

  return result.outcome === "success" && typeof versionPayload?.version === "string"
    ? versionPayload.version
    : null;
}

export async function loadSystemStatus(
  accessToken: string | null,
): Promise<SystemStatusModel> {
  const result = await fetchInternal("/internal/system-status", accessToken);
  const payload = record(result.data);

  return {
    services: [
      {
        service: "web",
        label: "Web",
        status: "operational",
        detail: "The Internal Console rendered successfully.",
      },
      simpleServiceCheck("api", "API", result, payload?.api, "ok"),
      simpleServiceCheck("database", "Database", result, payload?.database, "ready"),
      workerServiceCheck(result, payload?.worker),
    ],
    backendVersion: backendVersion(result, payload?.version),
  };
}
