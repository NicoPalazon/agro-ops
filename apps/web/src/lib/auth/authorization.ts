import { redirect } from "next/navigation";
import { apiBaseUrl } from "@/lib/supabase/env";
import { safeReturnPath } from "./return-path";
import { authenticatedAccessToken } from "./server";

export interface AuthorizationContext {
  usuario_id: string;
  organizacion_id: string;
  permisos: string[];
}

type AuthorizationResult =
  | { status: "authorized"; context: AuthorizationContext }
  | { status: "unauthenticated" }
  | { status: "denied" }
  | { status: "unavailable" };

const AUTHORIZATION_TIMEOUT_MS = 5_000;

function isAuthorizationContext(value: unknown): value is AuthorizationContext {
  if (typeof value !== "object" || value === null) return false;
  const record = value as Record<string, unknown>;
  return (
    typeof record.usuario_id === "string" &&
    typeof record.organizacion_id === "string" &&
    Array.isArray(record.permisos) &&
    record.permisos.every((permission) => typeof permission === "string")
  );
}

export async function loadAuthorizationContext(
  accessToken: string | null,
): Promise<AuthorizationResult> {
  if (!accessToken) return { status: "unauthenticated" };
  const baseUrl = apiBaseUrl();
  if (!baseUrl) return { status: "unavailable" };

  try {
    const response = await fetch(new URL("/me", baseUrl), {
      cache: "no-store",
      headers: { authorization: `Bearer ${accessToken}` },
      signal: AbortSignal.timeout(AUTHORIZATION_TIMEOUT_MS),
    });
    if (response.status === 401) return { status: "unauthenticated" };
    if (response.status === 403) return { status: "denied" };
    if (!response.ok) return { status: "unavailable" };

    const context: unknown = await response.json();
    return isAuthorizationContext(context)
      ? { status: "authorized", context }
      : { status: "unavailable" };
  } catch {
    return { status: "unavailable" };
  }
}

export async function requireCapability(
  permission: string,
  returnPath: string,
): Promise<AuthorizationContext> {
  const result = await loadAuthorizationContext(await authenticatedAccessToken());
  if (result.status === "unauthenticated") {
    redirect(`/login?next=${encodeURIComponent(safeReturnPath(returnPath))}`);
  }
  if (result.status === "denied") redirect("/acceso-denegado");
  if (result.status === "unavailable") redirect("/servicio-no-disponible");
  if (!result.context.permisos.includes(permission)) redirect("/acceso-denegado");
  return result.context;
}
