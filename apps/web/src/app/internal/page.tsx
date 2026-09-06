import styles from "./internal.module.css";

export default function InternalPage() {
  return (
    <>
      <section className={styles.heading}>
        <h1>Internal Console</h1>
        <p>
          Technical tools for verifying the services that support Agro Ops.
        </p>
      </section>

      <nav aria-label="Internal tools">
        <ul className={styles.toolList}>
          <li>
            <a className={styles.toolLink} href="/internal/system-status">
              <span className={styles.toolCopy}>
                <span className={styles.toolName}>System Status</span>
                <span className={styles.toolDescription}>
                  Live service health and backend version diagnostics.
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
