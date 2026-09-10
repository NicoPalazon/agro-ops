"use server";

import { revalidatePath } from "next/cache";
import {
  createRole,
  createUser,
  updateRole,
  updateUser,
} from "@/lib/access-administration/server";
import { authenticatedAccessToken } from "@/lib/auth/server";

async function accessToken(): Promise<string> {
  const token = await authenticatedAccessToken();
  if (!token) throw new Error("La sesión ya no está disponible.");
  return token;
}

function selected(formData: FormData, field: string): string[] {
  return formData.getAll(field).map(String);
}

export async function createUserAction(formData: FormData) {
  await createUser(await accessToken(), {
    correo_electronico: String(formData.get("correo_electronico") ?? ""),
    nombre_completo: String(formData.get("nombre_completo") ?? ""),
    roles_ids: selected(formData, "roles_ids"),
  });
  revalidatePath("/configuracion/usuarios");
}

export async function updateUserAction(userId: string, formData: FormData) {
  const user = await updateUser(await accessToken(), userId, {
    nombre_completo: String(formData.get("nombre_completo") ?? ""),
    roles_ids: selected(formData, "roles_ids"),
  });
  revalidatePath("/configuracion/usuarios");
  return user;
}

export async function setUserActiveAction(userId: string, active: boolean) {
  await updateUser(await accessToken(), userId, { activo: active });
  revalidatePath("/configuracion/usuarios");
}

export async function createRoleAction(formData: FormData) {
  await createRole(await accessToken(), {
    nombre: String(formData.get("nombre") ?? ""),
    descripcion: String(formData.get("descripcion") ?? ""),
    permisos: selected(formData, "permisos"),
  });
  revalidatePath("/configuracion/roles");
}

export async function updateRoleAction(roleId: string, formData: FormData) {
  await updateRole(await accessToken(), roleId, {
    nombre: String(formData.get("nombre") ?? ""),
    descripcion: String(formData.get("descripcion") ?? "") || null,
    permisos: selected(formData, "permisos"),
  });
  revalidatePath("/configuracion/roles");
}

export async function setRoleActiveAction(roleId: string, active: boolean) {
  await updateRole(await accessToken(), roleId, { activo: active });
  revalidatePath("/configuracion/roles");
  revalidatePath("/configuracion/usuarios");
}
