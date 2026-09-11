import type { TerritorialFicha } from "./territorial-map-presentation";
import styles from "./territorio.module.css";

interface TerritorialFichaPanelProps {
  ficha: TerritorialFicha | null;
}

const entityLabels = {
  establecimiento: "Establecimiento",
  lote_base: "Lote base",
  unidad_operativa: "Unidad operativa",
} as const;

export function TerritorialFichaPanel({ ficha }: TerritorialFichaPanelProps) {
  if (!ficha) {
    return (
      <aside className={styles.ficha} aria-live="polite">
        <p className={styles.fichaEyebrow}>Ficha territorial</p>
        <h2>Seleccione un polígono</h2>
        <p>La ficha muestra el contexto disponible sin alterar el historial territorial.</p>
      </aside>
    );
  }

  return (
    <aside className={styles.ficha} aria-live="polite">
      <p className={styles.fichaEyebrow}>{entityLabels[ficha.entityType]}</p>
      <h2>{ficha.nombre}</h2>
      <dl className={styles.fichaList}>
        <div>
          <dt>Código</dt>
          <dd>{ficha.codigo}</dd>
        </div>
        {ficha.establecimiento ? (
          <div>
            <dt>Establecimiento</dt>
            <dd>
              {ficha.establecimiento.codigo} · {ficha.establecimiento.nombre}
            </dd>
          </div>
        ) : null}
        {ficha.campana ? (
          <div>
            <dt>Campaña</dt>
            <dd>
              {ficha.campana.codigo} · {ficha.campana.nombre}
              <span className={styles.secondaryValue}>
                {ficha.campana.fecha_inicio} a {ficha.campana.fecha_fin}
              </span>
            </dd>
          </div>
        ) : null}
        {ficha.loteBaseIds ? (
          <div>
            <dt>Lotes base vinculados</dt>
            <dd>{ficha.loteBaseIds.length || "—"}</dd>
          </div>
        ) : null}
      </dl>

      {ficha.usos ? (
        <section className={styles.fichaSection}>
          <h3>Uso territorial</h3>
          {ficha.usos.length === 0 ? (
            <p>Sin usos territoriales registrados.</p>
          ) : (
            <ul className={styles.contextList}>
              {ficha.usos.map((use) => (
                <li key={use.id}>
                  <strong>{use.uso_codigo}</strong> · {use.uso_nombre}
                  <span>{use.fecha_inicio} a {use.fecha_fin}</span>
                </li>
              ))}
            </ul>
          )}
        </section>
      ) : null}

      {ficha.referenciasExternas?.length ? (
        <section className={styles.fichaSection}>
          <h3>Referencias externas</h3>
          <ul className={styles.contextList}>
            {ficha.referenciasExternas.map((reference) => (
              <li key={`${reference.sistema_externo}:${reference.external_id}`}>
                <strong>{reference.sistema_externo}</strong> · {reference.external_id}
              </li>
            ))}
          </ul>
        </section>
      ) : null}
    </aside>
  );
}
