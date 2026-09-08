import type { ReactNode } from "react";
import { requireCapability } from "@/lib/auth/authorization";
import styles from "./configuracion.module.css";

export default async function ConfigurationLayout({
  children,
}: Readonly<{ children: ReactNode }>) {
  await requireCapability("configuracion:administrar", "/configuracion/usuarios");

  return (
    <div className={styles.shell}>
      <header className={styles.header}>
        <a className={styles.brand} href="/configuracion/usuarios">
          Agro Ops · Configuración
        </a>
        <nav aria-label="Configuración">
          <a href="/configuracion/usuarios">Usuarios</a>
          <a href="/configuracion/roles">Roles</a>
        </nav>
      </header>
      <main className={styles.content}>{children}</main>
    </div>
  );
}
