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
        </ul>
      </nav>
    </>
  );
}
