import { authenticatedAccessToken } from "@/lib/auth/server";
import { loadRoles } from "@/lib/access-administration/server";
import {
  createRoleAction,
  setRoleActiveAction,
  updateRoleAction,
} from "../actions";
import styles from "../configuracion.module.css";

export default async function RolesPage() {
  const token = await authenticatedAccessToken();
  if (!token) throw new Error("La sesión ya no está disponible.");
  const { roles, permisos } = await loadRoles(token);

  return (
    <>
      <section className={styles.heading}>
        <p className={styles.eyebrow}>Accesos</p>
        <h1>Roles</h1>
        <p>Agrupá permisos canónicos y asigná esos roles a los usuarios.</p>
      </section>

      <section className={styles.panel}>
        <h2>Nuevo rol</h2>
        <form action={createRoleAction} className={styles.form}>
          <label>
            Nombre
            <input name="nombre" required />
          </label>
          <label>
            Descripción
            <input name="descripcion" />
          </label>
          <PermissionOptions permissions={permisos} />
          <button type="submit">Guardar</button>
        </form>
      </section>

      <section className={styles.list} aria-label="Roles de Agro Ops">
        {roles.map((role) => (
          <article className={styles.card} key={role.id}>
            <div className={styles.cardTitle}>
              <strong>{role.nombre}</strong>
              <span className={role.activo ? styles.active : styles.inactive}>
                {role.activo ? "Activo" : "Inactivo"}
              </span>
            </div>
            <form action={updateRoleAction.bind(null, role.id)} className={styles.form}>
              <label>
                Nombre
                <input defaultValue={role.nombre} name="nombre" required />
              </label>
              <label>
                Descripción
                <input defaultValue={role.descripcion ?? ""} name="descripcion" />
              </label>
              <PermissionOptions permissions={permisos} selected={role.permisos} />
              <button type="submit">Guardar</button>
            </form>
            <form action={setRoleActiveAction.bind(null, role.id, !role.activo)}>
              <button className={styles.secondary} type="submit">
                {role.activo ? "Desactivar" : "Activar"}
              </button>
            </form>
          </article>
        ))}
      </section>
    </>
  );
}

function PermissionOptions({
  permissions,
  selected = [],
}: {
  permissions: { codigo: string; nombre: string }[];
  selected?: string[];
}) {
  return (
    <fieldset>
      <legend>Permisos efectivos</legend>
      <div className={styles.options}>
        {permissions.map((permission) => (
          <label key={permission.codigo} title={permission.nombre}>
            <input
              defaultChecked={selected.includes(permission.codigo)}
              name="permisos"
              type="checkbox"
              value={permission.codigo}
            />
            {permission.codigo}
          </label>
        ))}
      </div>
    </fieldset>
  );
}
