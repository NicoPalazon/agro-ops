import type { Metadata } from "next";
import { safeReturnPath } from "@/lib/auth/return-path";
import { login } from "./actions";
import styles from "./login.module.css";
import { LoginSubmitButton } from "./submit-button";

export const metadata: Metadata = {
  title: "Iniciar sesión | Agro Ops",
};

interface LoginPageProps {
  searchParams: Promise<{ next?: string | string[]; error?: string | string[] }>;
}

export default async function LoginPage({ searchParams }: LoginPageProps) {
  const params = await searchParams;
  const returnPath = safeReturnPath(params.next);

  return (
    <main className={styles.page}>
      <section className={styles.card} aria-labelledby="login-title">
        <h1 id="login-title">Agro Ops</h1>
        <p>Iniciá sesión para acceder a Agro Ops.</p>
        {params.error === "invalid_credentials" ? (
          <p className={styles.error} role="alert">
            El correo electrónico o la contraseña no son correctos.
          </p>
        ) : null}
        <form action={login} className={styles.form}>
          <input name="next" type="hidden" value={returnPath} />
          <label>
            Correo electrónico
            <input autoComplete="email" name="email" required type="email" />
          </label>
          <label>
            Contraseña
            <input
              autoComplete="current-password"
              name="password"
              required
              type="password"
            />
          </label>
          <LoginSubmitButton />
        </form>
      </section>
    </main>
  );
}
