"use client";

import Link from "next/link";
import { type FormEvent, useMemo, useState } from "react";
import { createSupabaseBrowserClient } from "@/lib/supabase/browser";
import styles from "./invitation.module.css";

export default function AcceptInvitationPage() {
  const supabase = useMemo(() => createSupabaseBrowserClient(), []);
  const [message, setMessage] = useState<string | null>(null);
  const [completed, setCompleted] = useState(false);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    const password = String(form.get("password") ?? "");
    const confirmation = String(form.get("confirmation") ?? "");
    if (password.length < 8) {
      setMessage("La contraseña debe tener al menos 8 caracteres.");
      return;
    }
    if (password !== confirmation) {
      setMessage("Las contraseñas no coinciden.");
      return;
    }

    const { error } = await supabase.auth.updateUser({ password });
    if (error) {
      setMessage("La invitación venció o no es válida. Pedí una nueva invitación.");
      return;
    }
    await supabase.auth.signOut();
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
              <input name="password" type="password" minLength={8} required />
            </label>
            <label>
              Repetir contraseña
              <input name="confirmation" type="password" minLength={8} required />
            </label>
            {message ? <p role="alert">{message}</p> : null}
            <button type="submit">Guardar contraseña</button>
          </form>
        )}
      </section>
    </main>
  );
}
