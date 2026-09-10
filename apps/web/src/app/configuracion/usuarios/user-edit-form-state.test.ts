import { describe, expect, it } from "vitest";
import {
  failedSave,
  saveButtonState,
  saveFeedback,
  successfulSave,
  toggleRole,
  userEditValuesEqual,
  type UserEditValues,
} from "./user-edit-form-state";

const persisted: UserEditValues = { nombreCompleto: "Ana Pérez", roleIds: ["encargado"] };

describe("estado de edición de usuario", () => {
  it("considera limpio un formulario sin cambios", () => {
    expect(userEditValuesEqual(persisted, { ...persisted })).toBe(true);
    expect(saveButtonState(false, "idle")).toMatchObject({
      disabled: true,
      label: "Guardar cambios",
    });
  });

  it("detecta cambios de nombre y de roles", () => {
    expect(userEditValuesEqual(persisted, { ...persisted, nombreCompleto: "Ana Gómez" })).toBe(false);
    expect(userEditValuesEqual(persisted, { ...persisted, roleIds: ["administrador"] })).toBe(false);
    expect(saveButtonState(true, "idle")).toMatchObject({ disabled: false });
  });

  it("muestra el estado de carga y bloquea envíos duplicados durante el guardado", () => {
    expect(saveButtonState(true, "saving")).toEqual({
      disabled: true,
      loading: true,
      label: "Guardando…",
    });
  });

  it("confirma sólo el guardado exitoso y presenta un error reintentable al fallar", () => {
    const saved = successfulSave({ nombreCompleto: "Ana Gómez", roleIds: ["administrador"] });

    expect(saved.values).toEqual(saved.baseline);
    expect(saveFeedback(saved.saveState)).toBe("Cambios guardados");
    expect(saveButtonState(!userEditValuesEqual(saved.values, saved.baseline), saved.saveState))
      .toMatchObject({ disabled: true });

    const failed = failedSave({ nombreCompleto: "Ana Gómez", roleIds: ["administrador"] });
    expect(failed.values).toEqual({ nombreCompleto: "Ana Gómez", roleIds: ["administrador"] });
    expect(saveFeedback(failed.saveState)).toContain("No se pudieron guardar los cambios");
    expect(saveButtonState(true, failed.saveState)).toMatchObject({
      disabled: false,
      label: "Guardar cambios",
    });
  });

  it("normaliza roles para que el orden no habilite un guardado innecesario", () => {
    expect(userEditValuesEqual(
      { nombreCompleto: "Ana Pérez", roleIds: ["encargado", "administrador"] },
      { nombreCompleto: "Ana Pérez", roleIds: ["administrador", "encargado"] },
    )).toBe(true);
  });

  it("conserva la edición al volver a habilitar el rol para reintentar", () => {
    const edited = { ...persisted, roleIds: toggleRole(persisted.roleIds, "administrador", true) };

    expect(edited).toEqual({ nombreCompleto: "Ana Pérez", roleIds: ["administrador", "encargado"] });
    expect(userEditValuesEqual(persisted, edited)).toBe(false);
  });
});
