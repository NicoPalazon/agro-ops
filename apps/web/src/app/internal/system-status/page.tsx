import styles from "../internal.module.css";
import { loadSystemStatus, type DisplayStatus } from "./status";

const statusLabels: Record<DisplayStatus, string> = {
  operational: "Operational",
  degraded: "Degraded",
  stale: "Stale",
  unavailable: "Unavailable",
};

export default async function SystemStatusPage() {
  const status = await loadSystemStatus();

  return (
    <>
      <a className={styles.backLink} href="/internal">
        ← Internal Console
      </a>

      <section className={styles.heading}>
        <h1>System Status</h1>
        <p>
          Live checks for the local services that make up the Agro Ops Walking
          Skeleton.
        </p>
      </section>

      <ul className={styles.statusList} aria-label="Service status">
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
        <dt>Backend version</dt>
        <dd>{status.backendVersion ?? "Unavailable"}</dd>
      </dl>
    </>
  );
}
