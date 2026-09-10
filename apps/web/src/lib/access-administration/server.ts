import { apiBaseUrl } from "@/lib/supabase/env";

export interface RoleReference {
  id: string;
  nombre: string;
}

export interface UserSummary {
  id: string;
  nombre_completo: string;
  correo_electronico: string | null;
  activo: boolean;
  roles: RoleReference[];
}

export interface RoleSummary {
  id: string;
  nombre: string;
  descripcion: string | null;
  activo: boolean;
  permisos: string[];
}

export interface PermissionSummary {
  codigo: string;
  nombre: string;
}

export interface RolesResponse {
  roles: RoleSummary[];
  permisos: PermissionSummary[];
}

const REQUEST_TIMEOUT_MS = 10_000;

async function backendRequest<T>(
  accessToken: string,
  path: string,
  init?: RequestInit,
): Promise<T> {
  const baseUrl = apiBaseUrl();
  if (!baseUrl) throw new Error("La administración de accesos no está disponible.");
  const response = await fetch(new URL(path, baseUrl), {
    ...init,
    cache: "no-store",
    headers: {
      authorization: `Bearer ${accessToken}`,
      ...(init?.body ? { "content-type": "application/json" } : {}),
      ...init?.headers,
    },
    signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS),
  });
  if (!response.ok) {
    throw new Error("No se pudo completar la administración de accesos.");
  }
  return (await response.json()) as T;
}

export async function loadUsers(accessToken: string): Promise<UserSummary[]> {
  const response = await backendRequest<{ usuarios: UserSummary[] }>(
    accessToken,
    "/configuracion/usuarios",
  );
  return response.usuarios;
}

export function loadRoles(accessToken: string): Promise<RolesResponse> {
  return backendRequest(accessToken, "/configuracion/roles");
}

export function createUser(
  accessToken: string,
  input: {
    correo_electronico: string;
    nombre_completo: string;
    roles_ids: string[];
  },
): Promise<UserSummary> {
  return backendRequest(accessToken, "/configuracion/usuarios", {
    method: "POST",
    body: JSON.stringify(input),
  });
}

export function updateUser(
  accessToken: string,
  userId: string,
  input: { nombre_completo?: string; activo?: boolean; roles_ids?: string[] },
): Promise<UserSummary> {
  return backendRequest(accessToken, `/configuracion/usuarios/${userId}`, {
    method: "PATCH",
    body: JSON.stringify(input),
  });
}

export function createRole(
  accessToken: string,
  input: { nombre: string; descripcion?: string; permisos: string[] },
): Promise<RoleSummary> {
  return backendRequest(accessToken, "/configuracion/roles", {
    method: "POST",
    body: JSON.stringify(input),
  });
}

export function updateRole(
  accessToken: string,
  roleId: string,
  input: {
    nombre?: string;
    descripcion?: string | null;
    activo?: boolean;
    permisos?: string[];
  },
): Promise<RoleSummary> {
  return backendRequest(accessToken, `/configuracion/roles/${roleId}`, {
    method: "PATCH",
    body: JSON.stringify(input),
  });
}
