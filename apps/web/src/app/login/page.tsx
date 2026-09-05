import type { Metadata } from "next";
import { safeReturnPath } from "@/lib/auth/return-path";
import { login } from "./actions";
import styles from "./login.module.css";

export const metadata: Metadata = {
  title: "Sign in | Agro Ops",
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
        <p>Sign in to access the Internal Console.</p>
        {params.error === "invalid_credentials" ? (
          <p className={styles.error} role="alert">
            Your email or password was not accepted.
          </p>
        ) : null}
        <form action={login} className={styles.form}>
          <input name="next" type="hidden" value={returnPath} />
          <label>
            Email
            <input autoComplete="email" name="email" required type="email" />
          </label>
          <label>
            Password
            <input
              autoComplete="current-password"
              name="password"
              required
              type="password"
            />
          </label>
          <button type="submit">Sign in</button>
        </form>
      </section>
    </main>
  );
}
