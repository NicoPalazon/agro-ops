import type { Metadata } from "next";
import type { ReactNode } from "react";
import { requireCapability } from "@/lib/auth/authorization";
import { logout } from "../login/actions";
import { SignOutButton } from "./sign-out-button";
import styles from "./internal.module.css";

export const metadata: Metadata = {
  title: "Internal Console | Agro Ops",
  description: "Technical diagnostics for the Agro Ops stack.",
};

export default async function InternalLayout({
  children,
}: Readonly<{ children: ReactNode }>) {
  await requireCapability("consola_tecnica:ver", "/internal");

  return (
    <div className={styles.console}>
      <header className={styles.header}>
        <a className={styles.brand} href="/internal">
          Agro Ops · Consola técnica
        </a>
        <span className={styles.context}>Herramientas técnicas</span>
        <form action={logout}>
          <SignOutButton />
        </form>
      </header>
      <main className={styles.content}>{children}</main>
    </div>
  );
}
