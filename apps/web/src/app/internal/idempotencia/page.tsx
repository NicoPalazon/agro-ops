import { authenticatedAccessToken } from "@/lib/auth/server";
import styles from "../internal.module.css";
import { loadIdempotency } from "./idempotencia";

function timestamp(value: string): string {
  const date = new Date(value);
  return Number.isNaN(date.valueOf())
    ? value
    : new Intl.DateTimeFormat("es-AR", {
        dateStyle: "short",
        timeStyle: "medium",
        timeZone: "America/Argentina/Buenos_Aires",
      }).format(date);
}

export default async function IdempotencyPage() {
  const model = await loadIdempotency(await authenticatedAccessToken());

  return (
    <>
      <a className={styles.backLink} href="/internal">
        ← Consola técnica
      </a>

      <section className={styles.heading}>
        <h1>Idempotencia</h1>
        <p>
          Diagnóstico de comandos completados. Esta vista es de solo lectura y
          nunca muestra resultados de replay ni cuerpos de solicitud.
        </p>
      </section>

      {model.status === "unavailable" ? (
        <section className={styles.jobsNotice} role="status">
          <h2>No se pudo consultar la idempotencia</h2>
          <p>El diagnóstico de idempotencia no está disponible temporalmente.</p>
        </section>
      ) : model.records.length === 0 ? (
        <section className={styles.jobsNotice}>
          <h2>No hay registros de idempotencia</h2>
          <p>No hay comandos completados para mostrar.</p>
        </section>
      ) : (
        <div className={styles.jobsTableWrapper}>
          <table className={styles.jobsTable}>
            <caption className={styles.visuallyHidden}>
              Registros de idempotencia completados
            </caption>
            <thead>
              <tr>
                <th>Operación</th>
                <th>Clave de idempotencia</th>
                <th>Huella de solicitud</th>
                <th>Fecha de finalización</th>
                <th>Tamaño del resultado</th>
              </tr>
            </thead>
            <tbody>
              {model.records.map((record) => (
                <tr key={record.id}>
                  <td><code>{record.operacion}</code></td>
                  <td><code>{record.idempotency_key}</code></td>
                  <td><code>{record.request_sha256}</code></td>
                  <td>{timestamp(record.completado_en)}</td>
                  <td>{record.resultado_bytes} bytes</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </>
  );
}
