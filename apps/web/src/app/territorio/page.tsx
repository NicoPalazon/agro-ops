import { authenticatedAccessToken } from "@/lib/auth/server";
import { loadTerritorialMapData } from "@/lib/territory/api";
import { TerritorialMapExperience } from "./territorial-map-experience";
import styles from "./territorio.module.css";

export default async function TerritoryPage() {
  const result = await loadTerritorialMapData(await authenticatedAccessToken());

  if (result.status === "unavailable") {
    return (
      <main className={styles.statePage}>
        <section className={styles.notice} role="alert">
          <p className={styles.eyebrow}>Territorio</p>
          <h1>No se pudo cargar el territorio</h1>
          <p>
            La información territorial no está disponible temporalmente. No se
            muestra un mapa parcial.
          </p>
        </section>
      </main>
    );
  }

  return <TerritorialMapExperience data={result.data} />;
}
