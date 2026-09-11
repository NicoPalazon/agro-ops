import styles from "./territorio.module.css";

export default function TerritoryLoading() {
  return (
    <main className={styles.statePage} aria-busy="true">
      <section className={styles.notice} role="status">
        <p className={styles.eyebrow}>Territorio</p>
        <h1>Cargando mapa territorial</h1>
        <p>Consultando establecimientos, campañas y unidades operativas.</p>
      </section>
    </main>
  );
}
