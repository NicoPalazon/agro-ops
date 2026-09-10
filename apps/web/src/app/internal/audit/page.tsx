import { authenticatedAccessToken } from "@/lib/auth/server";
import styles from "../internal.module.css";
import { formatDiagnosticTimestamp } from "../diagnostic-format";
import { loadAudit } from "./audit";

function changeSummary(before: boolean, after: boolean): string {
  if (before && after) return "Estado anterior y posterior registrados";
  if (before) return "Estado anterior registrado";
  if (after) return "Estado posterior registrado";
  return "Sin snapshots";
}

export default async function AuditPage() {
  const model = await loadAudit(await authenticatedAccessToken());

  return (
    <>
      <a className={styles.backLink} href="/internal">
        ← Consola técnica
      </a>

      <section className={styles.heading}>
        <h1>Auditoría</h1>
        <p>
          Hechos inmutables de la organización. Esta vista es de solo lectura y
          muestra presencia de cambios, no snapshots de datos.
        </p>
      </section>

      {model.status === "unavailable" ? (
        <section className={styles.jobsNotice} role="status">
          <h2>No se pudo consultar la auditoría</h2>
          <p>El diagnóstico de auditoría no está disponible temporalmente.</p>
        </section>
      ) : model.events.length === 0 ? (
        <section className={styles.jobsNotice}>
          <h2>No hay eventos de auditoría</h2>
          <p>No hay hechos de auditoría para mostrar.</p>
        </section>
      ) : (
        <div className={styles.jobsTableWrapper}>
          <table className={styles.jobsTable}>
            <caption className={styles.visuallyHidden}>Eventos de auditoría</caption>
            <thead>
              <tr>
                <th>Fecha</th>
                <th>Acción</th>
                <th>Actor</th>
                <th>Entidad</th>
                <th>Referencia</th>
                <th>Cambio</th>
              </tr>
            </thead>
            <tbody>
              {model.events.map((event) => (
                <tr key={event.id}>
                  <td>{formatDiagnosticTimestamp(event.ocurrido_en)}</td>
                  <td><code>{event.accion}</code></td>
                  <td>
                    {event.actor_usuario_id
                      ? `${event.actor_tipo} · ${event.actor_usuario_id}`
                      : event.actor_tipo}
                  </td>
                  <td>
                    {[event.entidad_tipo, event.entidad_id].filter(Boolean).join(" · ")}
                  </td>
                  <td>{event.referencia ?? "—"}</td>
                  <td>
                    {changeSummary(
                      event.tiene_estado_anterior,
                      event.tiene_estado_posterior,
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </>
  );
}
