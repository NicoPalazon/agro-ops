export interface UserEditValues {
  nombreCompleto: string;
  roleIds: string[];
}

export type UserEditSaveState = "idle" | "saving" | "success" | "error";

export function saveButtonState(hasChanges: boolean, saveState: UserEditSaveState) {
  return {
    disabled: !hasChanges || saveState === "saving",
    loading: saveState === "saving",
    label: saveState === "saving" ? "Guardando…" : "Guardar cambios",
  };
}

export function saveFeedback(saveState: UserEditSaveState): string | null {
  if (saveState === "success") return "Cambios guardados";
  if (saveState === "error") {
    return "No se pudieron guardar los cambios. Revisá tu conexión e intentá nuevamente.";
  }
  return null;
}

export function successfulSave(values: UserEditValues) {
  return { baseline: values, values, saveState: "success" as const };
}

export function failedSave(values: UserEditValues) {
  return { values, saveState: "error" as const };
}

export function normalizedRoleIds(roleIds: string[]): string[] {
  return [...new Set(roleIds)].sort();
}

export function userEditValuesEqual(left: UserEditValues, right: UserEditValues): boolean {
  if (left.nombreCompleto !== right.nombreCompleto) return false;

  const leftRoleIds = normalizedRoleIds(left.roleIds);
  const rightRoleIds = normalizedRoleIds(right.roleIds);
  return (
    leftRoleIds.length === rightRoleIds.length &&
    leftRoleIds.every((roleId, index) => roleId === rightRoleIds[index])
  );
}

export function toggleRole(roleIds: string[], roleId: string, selected: boolean): string[] {
  return selected
    ? normalizedRoleIds([...roleIds, roleId])
    : roleIds.filter((assignedRoleId) => assignedRoleId !== roleId);
}
