import type { Metadata } from "next";
import type { ReactNode } from "react";
import { logout } from "../login/actions";
import { SignOutButton } from "./sign-out-button";
import styles from "./internal.module.css";

export const metadata: Metadata = {
  title: "Internal Console | Agro Ops",
  description: "Technical diagnostics for the Agro Ops stack.",
};

export default function InternalLayout({
  children,
}: Readonly<{ children: ReactNode }>) {
  return (
    <div className={styles.console}>
      <header className={styles.header}>
        <a className={styles.brand} href="/internal">
          Agro Ops · Internal Console
        </a>
        <span className={styles.context}>Technical tooling</span>
        <form action={logout}>
          <SignOutButton />
        </form>
      </header>
      <main className={styles.content}>{children}</main>
    </div>
  );
}
