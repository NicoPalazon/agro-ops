import { authenticatedAccessToken } from "@/lib/auth/server";
import { loadRoles, loadUsers } from "@/lib/access-administration/server";
import {
  createUserAction,
  setUserActiveAction,
  updateUserAction,
} from "../actions";
import styles from "../configuracion.module.css";

export default async function UsersPage() {
  const token = await authenticatedAccessToken();
  if (!token) throw new Error("La sesión ya no está disponible.");
  const [users, roleData] = await Promise.all([loadUsers(token), loadRoles(token)]);
  const activeRoles = roleData.roles.filter((role) => role.activo);

  return (
    <>
      <section className={styles.heading}>
        <p className={styles.eyebrow}>Accesos</p>
        <h1>Usuarios</h1>
        <p>Administrá quién puede ingresar a Agro Ops y qué roles tiene asignados.</p>
      </section>

      <section className={styles.panel}>
        <h2>Nuevo usuario</h2>
        <form action={createUserAction} className={styles.form}>
          <label>
            Correo electrónico
            <input name="correo_electronico" type="email" required />
          </label>
          <label>
            Nombre completo
            <input name="nombre_completo" required />
          </label>
          <fieldset>
            <legend>Roles asignados</legend>
            <div className={styles.options}>
              {activeRoles.map((role) => (
                <label key={role.id}>
                  <input name="roles_ids" type="checkbox" value={role.id} />
                  {role.nombre}
                </label>
              ))}
            </div>
          </fieldset>
          <button type="submit">Guardar</button>
        </form>
      </section>

      <section className={styles.list} aria-label="Usuarios de Agro Ops">
        {users.map((user) => (
          <article className={styles.card} key={user.id}>
            <div className={styles.cardTitle}>
              <div>
                <span className={styles.label}>Nombre</span>
                <strong>{user.nombre_completo}</strong>
                <span className={styles.label}>Correo electrÃ³nico</span>
                <span>{user.correo_electronico}</span>
              </div>
              <span className={user.activo ? styles.active : styles.inactive}>
                {user.activo ? "Habilitado" : "Deshabilitado"}
              </span>
            </div>
            <form action={updateUserAction.bind(null, user.id)} className={styles.form}>
              <label>
                Nombre completo
                <input defaultValue={user.nombre_completo} name="nombre_completo" required />
              </label>
              <fieldset>
                <legend>Roles asignados</legend>
                <div className={styles.options}>
                  {activeRoles.map((role) => (
                    <label key={role.id}>
                      <input
                        defaultChecked={user.roles.some((assigned) => assigned.id === role.id)}
                        name="roles_ids"
                        type="checkbox"
                        value={role.id}
                      />
                      {role.nombre}
                    </label>
                  ))}
                </div>
              </fieldset>
              <button type="submit">Guardar</button>
            </form>
            <form action={setUserActiveAction.bind(null, user.id, !user.activo)}>
              <button className={styles.secondary} type="submit">
                {user.activo ? "Deshabilitar" : "Habilitar"}
              </button>
            </form>
          </article>
        ))}
      </section>
    </>
  );
}
