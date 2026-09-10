import styles from "./internal.module.css";

export default function InternalPage() {
  return (
    <>
      <section className={styles.heading}>
        <h1>Consola técnica</h1>
        <p>Herramientas para verificar los servicios que sostienen Agro Ops.</p>
      </section>

      <nav aria-label="Internal tools">
        <ul className={styles.toolList}>
          <li>
            <a className={styles.toolLink} href="/internal/system-status">
              <span className={styles.toolCopy}>
                <span className={styles.toolName}>Estado del sistema</span>
                <span className={styles.toolDescription}>
                  Salud de servicios y versión actual del backend.
                </span>
              </span>
              <span className={styles.toolArrow} aria-hidden="true">
                →
              </span>
            </a>
          </li>
          <li>
            <a className={styles.toolLink} href="/internal/jobs">
              <span className={styles.toolCopy}>
                <span className={styles.toolName}>Jobs</span>
                <span className={styles.toolDescription}>
                  Estado, intentos y errores seguros de la cola PostgreSQL.
                </span>
              </span>
              <span className={styles.toolArrow} aria-hidden="true">
                →
              </span>
            </a>
          </li>
          <li>
            <a className={styles.toolLink} href="/internal/outbox">
              <span className={styles.toolCopy}>
                <span className={styles.toolName}>Outbox transaccional</span>
                <span className={styles.toolDescription}>
                  Eventos de integración y estado de su entrega por jobs.
                </span>
              </span>
              <span className={styles.toolArrow} aria-hidden="true">
                →
              </span>
            </a>
          </li>
          <li>
            <a className={styles.toolLink} href="/internal/audit">
              <span className={styles.toolCopy}>
                <span className={styles.toolName}>Auditoría</span>
                <span className={styles.toolDescription}>
                  Hechos de auditoría de la organización, sin exponer snapshots.
                </span>
              </span>
              <span className={styles.toolArrow} aria-hidden="true">
                →
              </span>
            </a>
          </li>
          <li>
            <a className={styles.toolLink} href="/internal/idempotencia">
              <span className={styles.toolCopy}>
                <span className={styles.toolName}>Idempotencia</span>
                <span className={styles.toolDescription}>
                  Coordinación de comandos completados, sin resultados de replay.
                </span>
              </span>
              <span className={styles.toolArrow} aria-hidden="true">
                →
              </span>
            </a>
          </li>
        </ul>
      </nav>
    </>
  );
}
