import { authenticatedAccessToken } from "@/lib/auth/server";
import styles from "../internal.module.css";
import { loadJobs, type JobState } from "./jobs";

const stateLabels: Record<JobState, string> = {
  pendiente: "Pendiente",
  ejecutando: "Ejecutando",
  completado: "Completado",
  agotado: "Agotado",
};

function timestamp(value: string | null): string {
  if (!value) return "—";
  const date = new Date(value);
  return Number.isNaN(date.valueOf())
    ? value
    : new Intl.DateTimeFormat("es-AR", {
        dateStyle: "short",
        timeStyle: "medium",
        timeZone: "America/Argentina/Buenos_Aires",
      }).format(date);
}

export default async function JobsPage() {
  const model = await loadJobs(await authenticatedAccessToken());

  return (
    <>
      <a className={styles.backLink} href="/internal">
        ← Consola técnica
      </a>

      <section className={styles.heading}>
        <h1>Jobs</h1>
        <p>
          Diagnóstico de la cola persistente de PostgreSQL. Esta vista no muestra
          los payloads ni permite cambiar el estado.
        </p>
      </section>

      {model.status === "unavailable" ? (
        <section className={styles.jobsNotice} role="status">
          <h2>No se pudo consultar la cola</h2>
          <p>El diagnóstico de jobs no está disponible temporalmente.</p>
        </section>
      ) : model.jobs.length === 0 ? (
        <section className={styles.jobsNotice}>
          <h2>La cola está vacía</h2>
          <p>No hay jobs persistidos para mostrar.</p>
        </section>
      ) : (
        <div className={styles.jobsTableWrapper}>
          <table className={styles.jobsTable}>
            <caption className={styles.visuallyHidden}>Jobs persistidos</caption>
            <thead>
              <tr>
                <th>Estado</th>
                <th>Tipo</th>
                <th>Intentos</th>
                <th>Próximo intento</th>
                <th>Bloqueo</th>
                <th>Última actualización</th>
                <th>Error seguro</th>
              </tr>
            </thead>
            <tbody>
              {model.jobs.map((job) => (
                <tr key={job.id}>
                  <td>
                    <span
                      className={`${styles.jobState} ${styles[`jobState_${job.estado}`]}`}
                    >
                      {stateLabels[job.estado]}
                    </span>
                  </td>
                  <td>
                    <code>{job.tipo}</code>
                  </td>
                  <td>
                    {job.intentos} / {job.max_intentos}
                  </td>
                  <td>{timestamp(job.next_attempt_at)}</td>
                  <td>
                    {job.bloqueado_en ? (
                      <span>
                        {timestamp(job.bloqueado_en)}
                        {job.bloqueado_por ? ` · ${job.bloqueado_por}` : ""}
                      </span>
                    ) : (
                      "—"
                    )}
                  </td>
                  <td>{timestamp(job.actualizado_en)}</td>
                  <td>{job.ultimo_error ?? "—"}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </>
  );
}
