import { authenticatedAccessToken } from "@/lib/auth/server";
import styles from "../internal.module.css";
import { formatDiagnosticTimestamp } from "../diagnostic-format";
import type { JobState } from "../jobs/jobs";
import { loadOutbox } from "./outbox";

const stateLabels: Record<JobState, string> = {
  pendiente: "Pendiente",
  ejecutando: "Ejecutando",
  completado: "Completado",
  agotado: "Agotado",
};

export default async function OutboxPage() {
  const model = await loadOutbox(await authenticatedAccessToken());

  return (
    <>
      <a className={styles.backLink} href="/internal">
        ← Consola técnica
      </a>

      <section className={styles.heading}>
        <h1>Outbox transaccional</h1>
        <p>
          Diagnóstico de eventos de integración y su entrega. Esta vista es de
          solo lectura y nunca muestra payloads.
        </p>
      </section>

      {model.status === "unavailable" ? (
        <section className={styles.jobsNotice} role="status">
          <h2>No se pudo consultar el outbox</h2>
          <p>El diagnóstico de integraciones no está disponible temporalmente.</p>
        </section>
      ) : model.events.length === 0 ? (
        <section className={styles.jobsNotice}>
          <h2>No hay eventos de integración</h2>
          <p>El outbox transaccional está vacío.</p>
        </section>
      ) : (
        <div className={styles.jobsTableWrapper}>
          <table className={styles.jobsTable}>
            <caption className={styles.visuallyHidden}>
              Eventos del outbox transaccional
            </caption>
            <thead>
              <tr>
                <th>Estado de entrega</th>
                <th>Destino</th>
                <th>Evento</th>
                <th>Entidad / referencia</th>
                <th>Intentos</th>
                <th>Próximo intento</th>
                <th>Última actualización</th>
                <th>Error seguro</th>
              </tr>
            </thead>
            <tbody>
              {model.events.map((event) => (
                <tr key={event.id}>
                  <td>
                    <span
                      className={`${styles.jobState} ${styles[`jobState_${event.estado}`]}`}
                    >
                      {stateLabels[event.estado]}
                    </span>
                  </td>
                  <td>
                    <code>{event.destino}</code>
                  </td>
                  <td>
                    <code>{event.evento_tipo}</code>
                  </td>
                  <td>
                    {[event.entidad_tipo, event.entidad_id, event.referencia]
                      .filter(Boolean)
                      .join(" · ") || "—"}
                  </td>
                  <td>
                    {event.intentos} / {event.max_intentos}
                  </td>
                  <td>{formatDiagnosticTimestamp(event.next_attempt_at)}</td>
                  <td>{formatDiagnosticTimestamp(event.actualizado_en)}</td>
                  <td>{event.ultimo_error ?? "—"}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </>
  );
}
