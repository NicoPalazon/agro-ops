"use client";

import Link from "next/link";
import { type FormEvent, useEffect, useMemo, useRef, useState } from "react";
import { createSupabaseInvitationClient } from "@/lib/supabase/browser";
import {
  establishInvitationSession,
  invitationErrorMessage,
  type InvitationErrorKind,
  PASSWORD_POLICY_MESSAGE,
  updateInvitationPassword,
} from "./invitation-auth";
import styles from "./invitation.module.css";

type InvitationState =
  | { status: "checking" }
  | { status: "ready"; invitedUserId: string }
  | { status: "failed"; error: Exclude<InvitationErrorKind, "password-policy"> };

export function InvitationAlert({ kind }: { kind: InvitationErrorKind }) {
  return <p role="alert">{invitationErrorMessage(kind)}</p>;
}

export default function AcceptInvitationPage() {
  const supabase = useMemo(() => createSupabaseInvitationClient(), []);
  const initialization = useRef<ReturnType<typeof establishInvitationSession> | null>(null);
  const [invitation, setInvitation] = useState<InvitationState>({ status: "checking" });
  const [message, setMessage] = useState<string | null>(null);
  const [errorKind, setErrorKind] = useState<InvitationErrorKind | null>(null);
  const [completed, setCompleted] = useState(false);
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => {
    let active = true;

    initialization.current ??= establishInvitationSession(
      supabase.auth,
      window.location.hash,
      () => {
        window.history.replaceState(
          window.history.state,
          "",
          `${window.location.pathname}${window.location.search}`,
        );
      },
    );

    void initialization.current.then((result) => {
      if (!active) {
        return;
      }

      if (result.ok) {
        setInvitation({ status: "ready", invitedUserId: result.invitedUserId });
        return;
      }

      setInvitation({ status: "failed", error: result.error });
      setErrorKind(result.error);
    });

    return () => {
      active = false;
    };
  }, [supabase.auth]);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    const password = String(form.get("password") ?? "");
    const confirmation = String(form.get("confirmation") ?? "");
    setMessage(null);
    setErrorKind(null);

    if (invitation.status !== "ready") {
      setErrorKind(invitation.status === "failed" ? invitation.error : "unexpected");
      return;
    }

    if (password.length < 12) {
      setErrorKind("password-policy");
      return;
    }

    if (password !== confirmation) {
      setMessage("Las contraseñas no coinciden.");
      return;
    }

    setSubmitting(true);
    const result = await updateInvitationPassword(
      supabase.auth,
      invitation.invitedUserId,
      password,
    );
    setSubmitting(false);

    if (!result.ok) {
      setErrorKind(result.error);
      return;
    }

    setCompleted(true);
    setMessage(null);
  }

  return (
    <main className={styles.page}>
      <section className={styles.card}>
        <p className={styles.eyebrow}>Agro Ops</p>
        <h1>Aceptar invitación</h1>
        {completed ? (
          <>
            <p>Tu contraseña quedó guardada. Ya podés iniciar sesión.</p>
            <Link href="/login">Ir a iniciar sesión</Link>
          </>
        ) : (
          <form onSubmit={submit}>
            <label>
              Contraseña
              <input name="password" type="password" minLength={12} required />
            </label>
            <label>
              Repetir contraseña
              <input name="confirmation" type="password" minLength={12} required />
            </label>
            <p>{PASSWORD_POLICY_MESSAGE}</p>
            {message ? <p role="alert">{message}</p> : null}
            {errorKind ? <InvitationAlert kind={errorKind} /> : null}
            <button
              type="submit"
              disabled={invitation.status !== "ready" || submitting}
            >
              {invitation.status === "checking"
                ? "Verificando invitación…"
                : submitting
                  ? "Guardando…"
                  : "Guardar contraseña"}
            </button>
          </form>
        )}
      </section>
    </main>
  );
}
