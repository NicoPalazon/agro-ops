"use client";

import { type FormEvent, useEffect, useState } from "react";
import type { RoleReference, UserSummary } from "@/lib/access-administration/server";
import {
  failedSave,
  saveButtonState,
  saveFeedback,
  successfulSave,
  toggleRole,
  userEditValuesEqual,
  type UserEditSaveState,
  type UserEditValues,
} from "./user-edit-form-state";
import styles from "../configuracion.module.css";

interface UserEditFormProps {
  user: UserSummary;
  activeRoles: RoleReference[];
  action: (formData: FormData) => Promise<UserSummary>;
}

function valuesFor(user: UserSummary, activeRoles: RoleReference[]): UserEditValues {
  const activeRoleIds = new Set(activeRoles.map((role) => role.id));
  return {
    nombreCompleto: user.nombre_completo,
    roleIds: user.roles.filter((role) => activeRoleIds.has(role.id)).map((role) => role.id),
  };
}

export function UserEditForm({ user, activeRoles, action }: UserEditFormProps) {
  const initialValues = valuesFor(user, activeRoles);
  const [baseline, setBaseline] = useState(initialValues);
  const [values, setValues] = useState(initialValues);
  const [saveState, setSaveState] = useState<UserEditSaveState>("idle");
  const isSaving = saveState === "saving";
  const hasChanges = !userEditValuesEqual(values, baseline);
  const buttonState = saveButtonState(hasChanges, saveState);
  const feedback = saveFeedback(saveState);

  useEffect(() => {
    if (saveState !== "success") return;

    const timeout = window.setTimeout(() => setSaveState("idle"), 3_000);
    return () => window.clearTimeout(timeout);
  }, [saveState]);

  function edit(nextValues: UserEditValues) {
    setValues(nextValues);
    if (saveState !== "saving") setSaveState("idle");
  }

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (isSaving || !hasChanges) return;

    setSaveState("saving");
    try {
      const savedUser = await action(new FormData(event.currentTarget));
      const savedValues = valuesFor(savedUser, activeRoles);
      const result = successfulSave(savedValues);
      setBaseline(result.baseline);
      setValues(result.values);
      setSaveState(result.saveState);
    } catch {
      const result = failedSave(values);
      setValues(result.values);
      setSaveState(result.saveState);
    }
  }

  return (
    <form onSubmit={submit} className={styles.form}>
      <label>
        Nombre completo
        <input
          disabled={isSaving}
          name="nombre_completo"
          onChange={(event) => edit({ ...values, nombreCompleto: event.target.value })}
          required
          value={values.nombreCompleto}
        />
      </label>
      <fieldset>
        <legend>Roles asignados</legend>
        <div className={styles.options}>
          {activeRoles.map((role) => (
            <label key={role.id}>
              <input
                checked={values.roleIds.includes(role.id)}
                disabled={isSaving}
                name="roles_ids"
                onChange={(event) =>
                  edit({
                    ...values,
                    roleIds: toggleRole(values.roleIds, role.id, event.target.checked),
                  })
                }
                type="checkbox"
                value={role.id}
              />
              {role.nombre}
            </label>
          ))}
        </div>
      </fieldset>
      <div className={styles.saveFeedback}>
        <button disabled={buttonState.disabled} type="submit">
          {buttonState.loading ? (
            <>
              <span aria-hidden="true" className={styles.spinner} /> Guardando…
            </>
          ) : (
            buttonState.label
          )}
        </button>
        {saveState === "success" ? (
          <p className={styles.saveSuccess} role="status">
            <span aria-hidden="true">✓</span> {feedback}
          </p>
        ) : null}
        {saveState === "error" ? (
          <p className={styles.saveError} role="alert">
            {feedback}
          </p>
        ) : null}
      </div>
    </form>
  );
}
