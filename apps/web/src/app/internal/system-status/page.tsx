import styles from "../internal.module.css";
import { authenticatedAccessToken } from "@/lib/auth/server";
import { loadSystemStatus, type DisplayStatus } from "./status";

const statusLabels: Record<DisplayStatus, string> = {
  operational: "Operativo",
  degraded: "Degradado",
  stale: "Desactualizado",
  unavailable: "No disponible",
};

export default async function SystemStatusPage() {
  const status = await loadSystemStatus(await authenticatedAccessToken());

  return (
    <>
      <a className={styles.backLink} href="/internal">
        ← Consola técnica
      </a>

      <section className={styles.heading}>
        <h1>Estado del sistema</h1>
        <p>Verificaciones actuales de los servicios que sostienen Agro Ops.</p>
      </section>

      <ul className={styles.statusList} aria-label="Estado de servicios">
        {status.services.map((service) => (
          <li
            className={styles.statusRow}
            data-service={service.service}
            data-status={service.status}
            key={service.service}
          >
            <span className={styles.serviceCopy}>
              <span className={styles.serviceName}>{service.label}</span>
              <span className={styles.serviceDetail}>{service.detail}</span>
            </span>
            <span
              className={`${styles.status} ${styles[service.status]}`}
              aria-label={`${service.label}: ${statusLabels[service.status]}`}
            >
              {statusLabels[service.status]}
            </span>
          </li>
        ))}
      </ul>

      <dl className={styles.metadata}>
        <dt>Versión del backend</dt>
        <dd>{status.backendVersion ?? "No disponible"}</dd>
      </dl>
    </>
  );
}
