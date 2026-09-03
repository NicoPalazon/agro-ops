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

async function fetchInternal(path: string): Promise<FetchResult> {
  const apiBaseUrl = process.env.API_BASE_URL;

  if (!apiBaseUrl) {
    return { outcome: "unavailable" };
  }

  try {
    const response = await fetch(new URL(path, apiBaseUrl), {
      cache: "no-store",
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
  expectedStatus: string,
): ServiceCheck {
  if (result.outcome === "unavailable") {
    return { service, label, status: "unavailable", detail: unavailableDetail };
  }

  const payload = record(result.data);

  if (result.outcome === "success" && payload?.status === expectedStatus) {
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

function workerServiceCheck(result: FetchResult): ServiceCheck {
  if (result.outcome === "unavailable") {
    return {
      service: "worker",
      label: "Worker",
      status: "unavailable",
      detail: unavailableDetail,
    };
  }

  const payload = record(result.data);
  const status = payload?.status;
  const lastSeenAt =
    typeof payload?.last_seen_at === "string" ? payload.last_seen_at : null;

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

function backendVersion(result: FetchResult): string | null {
  const payload = record(result.data);

  return result.outcome === "success" && typeof payload?.version === "string"
    ? payload.version
    : null;
}

export async function loadSystemStatus(): Promise<SystemStatusModel> {
  const [health, readiness, worker, version] = await Promise.all([
    fetchInternal("/health"),
    fetchInternal("/ready"),
    fetchInternal("/internal/worker/status"),
    fetchInternal("/version"),
  ]);

  return {
    services: [
      {
        service: "web",
        label: "Web",
        status: "operational",
        detail: "The Internal Console rendered successfully.",
      },
      simpleServiceCheck("api", "API", health, "ok"),
      simpleServiceCheck("database", "Database", readiness, "ready"),
      workerServiceCheck(worker),
    ],
    backendVersion: backendVersion(version),
  };
}
