# Etapa 2, Slice 2.1: diseño físico de organización, usuarios, roles y permisos

## 1. Alcance y decisiones rectoras

Este documento materializa en PostgreSQL los conceptos aprobados en la Etapa 1.5: `Organization`, `User`, `ExternalAuthIdentity`, `Role`, `Capability`, `UserRole` y `RoleCapability`. Los nombres físicos y funcionales se expresan en español, sin cambiar su significado conceptual.

Las únicas relaciones de este slice son:

1. `organizaciones`;
2. `usuarios`;
3. `identidades_autenticacion_externas`;
4. `roles`;
5. `permisos`;
6. `usuarios_roles`;
7. `roles_permisos`.

Decisiones generales:

- Cada entidad y cada episodio histórico de una asociación tiene una identidad interna `uuid` con `DEFAULT gen_random_uuid()`.
- Los instantes reales usan `timestamptz` y los intervalos de vigencia son semiabiertos: `[vigente_desde, vigente_hasta)`. Un extremo final nulo significa vigencia actual sin límite conocido.
- V1 opera una sola organización, pero esa condición es de aprovisionamiento, no una clave artificial ni una restricción de singleton. No se usa un UUID mágico, una bandera singleton, RLS multi-tenant ni una clave de organización repetida en todas las tablas.
- Supabase Auth autentica y mantiene sesiones. Agro Ops resuelve el `sub` estable de Supabase a un `usuarios.id`; el correo electrónico no se almacena en este slice ni participa de ninguna clave o decisión de autorización.
- Los permisos son la unidad semántica de autorización. Los roles sólo los agrupan. El backend deberá consultar permisos efectivos por `permisos.codigo`, nunca nombres de roles.
- No hay borrado en cascada. Todos los FK usan `ON UPDATE RESTRICT ON DELETE RESTRICT` de forma explícita.
- Los nombres, rótulos y descripciones son datos presentacionales mutables. Los UUID, los sujetos externos, los códigos de permiso y los episodios históricos son identidad o historia y no se reescriben.
- Este diseño no agrega `audit_events`, idempotencia, jobs, outbox, documentos, referencias externas genéricas ni entidades de dominio de etapas posteriores.

PostgreSQL ya está elegido como fuente de verdad y Supabase como proveedor de Auth en staging y producción. La migración siguiente deberá usar las convenciones SQL existentes y probarse contra PostgreSQL/PostGIS real.

## 2. Diseño relación por relación

### 2.1. `organizaciones`

**Propósito.** Representa a la empresa dueña de los datos operativos. V1 aprovisiona exactamente una organización operativa, pero la relación conserva identidad interna normal y no codifica una plataforma SaaS.

| Columna | Tipo PostgreSQL | Nulabilidad / default | Semántica |
|---|---|---|---|
| `id` | `uuid` | `NOT NULL DEFAULT gen_random_uuid()` | Identidad interna de Agro Ops. |
| `nombre` | `text` | `NOT NULL` | Nombre funcional o de fantasía mostrado en la aplicación. |
| `activa` | `boolean` | `NOT NULL DEFAULT true` | Habilita o suspende globalmente la operación de la organización. |
| `creada_en` | `timestamptz` | `NOT NULL DEFAULT now()` | Instante de creación. |

Claves y restricciones:

- PK: `pk_organizaciones (id)`.
- Claves candidatas: sólo `id`. El nombre no es una identidad legal ni necesariamente único.
- CHECK `ck_organizaciones_nombre`: `nombre = btrim(nombre) AND nombre <> ''`.
- No hay restricción para forzar una sola fila ni una sola fila activa.
- No hay índices adicionales: el PK cubre la referencia y el volumen V1 no justifica índices especulativos.
- No se agregan todavía CUIT, razón social, domicilio fiscal u otros datos legales: este slice no tiene un caso de uso que los consuma y `nombre` alcanza para identificar visualmente a la única organización operativa.

Ciclo de vida e historia:

- `nombre` es perfil mutable.
- La suspensión se expresa con `activa = false`; no se borra la fila.
- Una organización inactiva hace fallar en forma cerrada la autorización de sus usuarios, aunque sus asignaciones sigan históricamente vigentes.
- El runtime no ofrece borrado. `ON DELETE RESTRICT` en hijos impide eliminar una organización que ya tenga identidad o configuración relacionada. Una corrección privilegiada de una fila nunca utilizada queda fuera de la API normal.
- La evolución de `nombre` y `activa` será observable por el futuro `AuditEvent`; este slice conserva estado actual, no versiones del perfil.

### 2.2. `usuarios`

**Propósito.** Es la identidad interna de la persona/cuenta en Agro Ops para autorización, autoría y futuras referencias de auditoría. No autentica credenciales ni sesiones.

| Columna | Tipo PostgreSQL | Nulabilidad / default | Semántica |
|---|---|---|---|
| `id` | `uuid` | `NOT NULL DEFAULT gen_random_uuid()` | Identidad interna estable. |
| `organizacion_id` | `uuid` | `NOT NULL` | Organización a la que pertenece el usuario. |
| `nombre_completo` | `text` | `NOT NULL` | Nombre humano mostrado en la aplicación. |
| `activo` | `boolean` | `NOT NULL DEFAULT true` | Habilitación interna de la cuenta. |
| `creado_en` | `timestamptz` | `NOT NULL DEFAULT now()` | Instante de creación interna. |

Claves y restricciones:

- PK: `pk_usuarios (id)`.
- Claves candidatas: sólo `id`.
- FK `fk_usuarios_organizacion`: `organizacion_id -> organizaciones(id) ON UPDATE RESTRICT ON DELETE RESTRICT`.
- CHECK `ck_usuarios_nombre_completo`: `nombre_completo = btrim(nombre_completo) AND nombre_completo <> ''`.
- El trigger específico `trg_usuarios_organizacion_inmutable` rechaza cambios de `organizacion_id`. Un traslado entre empresas tendría semántica de otra identidad interna y no existe en V1.
- No hay columna ni restricción de email.
- No se agrega índice por `organizacion_id`: hay una organización y aproximadamente diez usuarios; el FK no crea un índice automáticamente, pero hoy no existe un patrón que lo justifique.

Ciclo de vida e historia:

- `nombre_completo` y `activo` son estado mutable.
- Deshabilitar usa `activo = false`; las autorías y asignaciones históricas siguen apuntando al mismo UUID.
- Un usuario inactivo no obtiene permisos efectivos, aunque tenga intervalos abiertos en `usuarios_roles`.
- La API normal no borra usuarios. Los FK restrictivos evitan destruir usuarios referenciados por identidades o historial de roles.
- No existe autorregistro público: crear y activar usuarios pertenece al bootstrap controlado o a una futura operación administrativa autorizada.

### 2.3. `identidades_autenticacion_externas`

**Propósito.** Vincula un sujeto estable emitido por un proveedor de autenticación con un usuario interno. Para V1, el valor de `proveedor` usado por el sistema es `supabase` y `sujeto_proveedor` es el UUID recibido como `sub`/`id` de Supabase Auth.

| Columna | Tipo PostgreSQL | Nulabilidad / default | Semántica |
|---|---|---|---|
| `id` | `uuid` | `NOT NULL DEFAULT gen_random_uuid()` | Identidad interna del vínculo externo. |
| `usuario_id` | `uuid` | `NOT NULL` | Usuario interno vinculado. |
| `proveedor` | `text` | `NOT NULL` | Namespace canónico del proveedor; V1 escribe `supabase`. |
| `sujeto_proveedor` | `uuid` | `NOT NULL` | Identificador estable del usuario en Supabase Auth. |
| `vinculada_en` | `timestamptz` | `NOT NULL DEFAULT now()` | Inicio del vínculo. |
| `desvinculada_en` | `timestamptz` | `NULL` | Fin exclusivo; nulo significa vínculo actual. |

Claves y restricciones:

- PK: `pk_identidades_autenticacion_externas (id)`.
- Clave candidata natural: `(proveedor, sujeto_proveedor)`, materializada por `uq_identidades_auth_proveedor_sujeto`. Un sujeto externo no puede cambiar de usuario interno ni reutilizarse después de una desvinculación.
- Otra clave candidata semántica de episodio es `(usuario_id, vinculada_en)`; el constraint de exclusión impide dos episodios del usuario con el mismo inicio.
- FK `fk_identidades_auth_usuario`: `usuario_id -> usuarios(id) ON UPDATE RESTRICT ON DELETE RESTRICT`.
- CHECK `ck_identidades_auth_proveedor`: `proveedor = btrim(proveedor) AND proveedor ~ '^[a-z][a-z0-9_]*$'`.
- CHECK `ck_identidades_auth_vigencia`: `desvinculada_en IS NULL OR desvinculada_en > vinculada_en`.
- EXCLUDE `excl_identidades_auth_usuario_vigencia` usando GiST: `(usuario_id WITH =, tstzrange(vinculada_en, desvinculada_en, '[)') WITH &&)`. Un usuario no puede tener dos identidades externas vigentes en intervalos superpuestos.
- No se agrega un componente `issuer`, project ID, ambiente o URL a la clave. Cada base local, staging o producción se vincula a un solo despliegue de Auth y esos ambientes ya están físicamente separados. Añadir ese namespace no distinguiría nada dentro de una base V1. La pareja `(proveedor, sujeto_proveedor)` es suficiente.
- No se necesita índice adicional para autenticación: el B-tree de `uq_identidades_auth_proveedor_sujeto` resuelve la búsqueda exacta y luego se verifica `desvinculada_en IS NULL`.

Ciclo de vida e historia:

- Autenticar exige encontrar la pareja exacta `('supabase', sub)` con `desvinculada_en IS NULL`, y además que `usuarios.activo` y `organizaciones.activa` sean verdaderos.
- Desvincular actualiza una única vez `desvinculada_en`; no borra la fila. Una nueva cuenta de Supabase para la misma persona crea otra fila con otro sujeto.
- `trg_identidades_auth_historia` rechaza `DELETE`, cambios de `usuario_id`, `proveedor`, `sujeto_proveedor` o `vinculada_en`, reaperturas y segundas modificaciones de `desvinculada_en`. Sólo permite la transición de fin nulo a un instante válido.
- Revocar el vínculo no borra ni desactiva el usuario interno. Deshabilitar temporalmente a la persona normalmente se hace con `usuarios.activo = false`; la desvinculación se reserva para retirar de manera terminal ese sujeto.
- La relación no replica email, claims mutables, tokens ni datos de sesión.

### 2.4. `roles`

**Propósito.** Agrupa permisos con un nombre administrable dentro de la organización. El nombre ayuda a administrar, pero no es una condición de autorización.

| Columna | Tipo PostgreSQL | Nulabilidad / default | Semántica |
|---|---|---|---|
| `id` | `uuid` | `NOT NULL DEFAULT gen_random_uuid()` | Identidad interna del rol. |
| `organizacion_id` | `uuid` | `NOT NULL` | Organización dueña de la configuración del rol. |
| `nombre` | `text` | `NOT NULL` | Rótulo administrable del rol. |
| `descripcion` | `text` | `NULL` | Explicación funcional opcional. |
| `activo` | `boolean` | `NOT NULL DEFAULT true` | Permite retirar el rol sin borrar su historia. |
| `creado_en` | `timestamptz` | `NOT NULL DEFAULT now()` | Instante de creación. |

Claves y restricciones:

- PK: `pk_roles (id)`.
- Clave candidata administrativa: `(organizacion_id, nombre)`, materializada por `uq_roles_organizacion_nombre`. La comparación es la comparación exacta normal de `text`; no se agrega `citext` ni otra extensión.
- FK `fk_roles_organizacion`: `organizacion_id -> organizaciones(id) ON UPDATE RESTRICT ON DELETE RESTRICT`.
- CHECK `ck_roles_nombre`: `nombre = btrim(nombre) AND nombre <> ''`.
- CHECK `ck_roles_descripcion`: `descripcion IS NULL OR btrim(descripcion) <> ''`.
- `trg_roles_organizacion_inmutable` rechaza cambios de `organizacion_id`.
- No hay índice adicional; el UNIQUE ya cubre listar/buscar roles por organización y nombre.

Alcance organizacional:

- El rol sí pertenece a una organización porque es configuración administrable de esa empresa, no catálogo global del producto.
- Esta única FK no convierte V1 en SaaS: no se modelan tenants, membresías multiempresa ni aislamiento por RLS. El usuario pertenece exactamente a una organización y el rol también.
- `trg_usuarios_roles_misma_organizacion`, ejecutado `BEFORE INSERT OR UPDATE OF usuario_id, rol_id`, valida en la base que el usuario y el rol de toda asignación compartan `organizacion_id`. Se usa un trigger de integridad específico porque duplicar `organizacion_id` en `usuarios_roles` introduciría la dependencia transitiva `usuario_id -> organizacion_id` y rompería 3NF/BCNF. Las columnas de pertenencia son inmutables, por lo que validar al insertar basta; el trigger también cubre cualquier cambio de participantes aunque el trigger histórico normalmente lo rechace.

Ciclo de vida e historia:

- `nombre`, `descripcion` y `activo` son configuración mutable.
- `activo = false` retira inmediatamente todos sus permisos efectivos sin cerrar ni borrar episodios históricos.
- La API normal no borra roles. Los FK desde el historial usan `RESTRICT`.
- Renombrar un rol no altera autorización porque ninguna comprobación usa `nombre`.

### 2.5. `permisos`

**Propósito.** Catálogo canónico y global del producto con las capacidades que el backend puede exigir.

| Columna | Tipo PostgreSQL | Nulabilidad / default | Semántica |
|---|---|---|---|
| `id` | `uuid` | `NOT NULL DEFAULT gen_random_uuid()` | Identidad interna. |
| `codigo` | `text` | `NOT NULL` | Código canónico estable y consumible por máquina. |
| `nombre` | `text` | `NOT NULL` | Rótulo funcional en español. |
| `descripcion` | `text` | `NULL` | Alcance funcional opcional. |
| `activo` | `boolean` | `NOT NULL DEFAULT true` | Retiro no destructivo del permiso. |
| `creado_en` | `timestamptz` | `NOT NULL DEFAULT now()` | Instante de incorporación al catálogo. |

Claves y restricciones:

- PK: `pk_permisos (id)`.
- Clave candidata: `codigo`, materializada por `uq_permisos_codigo`.
- CHECK `ck_permisos_codigo`: `codigo ~ '^[a-z][a-z0-9_]*:[a-z][a-z0-9_]*$'`. Obliga formato `seccion:accion`, minúsculas ASCII y sin espacios o acentos.
- CHECK `ck_permisos_nombre`: `nombre = btrim(nombre) AND nombre <> ''`.
- CHECK `ck_permisos_descripcion`: `descripcion IS NULL OR btrim(descripcion) <> ''`.
- `trg_permisos_codigo_inmutable` rechaza cambios de `codigo`; cambiar el significado crea un permiso nuevo y desactiva el anterior.
- El UNIQUE de `codigo` es también el índice de búsqueda; no se agrega otro.

Se almacena **un solo código canónico**, no columnas redundantes `seccion` y `accion`. El código ya determina ambas partes por su sintaxis. Guardar las tres representaciones agregaría estado derivado que debería mantenerse sincronizado, sin una necesidad de consulta demostrada. Si en el futuro se requiere filtrar por sección, puede extraerse el prefijo o revisarse el diseño con evidencia; V1 autoriza por el código completo.

Ciclo de vida e historia:

- `nombre` y `descripcion` pueden mejorar sin cambiar semántica de autorización.
- El código es inmutable.
- Un permiso retirado pasa a `activo = false`; no se borra y deja de ser efectivo aunque exista un `roles_permisos` vigente.
- Los permisos son definidos por el producto y sembrados por migración; no son texto libre creado por cada organización.

### 2.6. `usuarios_roles`

**Propósito.** Registra cada episodio durante el cual un usuario tuvo un rol. No representa sólo el estado actual.

| Columna | Tipo PostgreSQL | Nulabilidad / default | Semántica |
|---|---|---|---|
| `id` | `uuid` | `NOT NULL DEFAULT gen_random_uuid()` | Identidad estable del episodio. |
| `usuario_id` | `uuid` | `NOT NULL` | Usuario asignado. |
| `rol_id` | `uuid` | `NOT NULL` | Rol otorgado. |
| `vigente_desde` | `timestamptz` | `NOT NULL DEFAULT now()` | Inicio inclusivo. |
| `vigente_hasta` | `timestamptz` | `NULL` | Fin exclusivo; nulo significa asignación actual. |

Claves y restricciones:

- PK: `pk_usuarios_roles (id)`.
- Clave candidata semántica: `(usuario_id, rol_id, vigente_desde)`. El EXCLUDE impide dos episodios del mismo par con el mismo comienzo; no se agrega un UNIQUE B-tree redundante.
- FK `fk_usuarios_roles_usuario`: `usuario_id -> usuarios(id) ON UPDATE RESTRICT ON DELETE RESTRICT`.
- FK `fk_usuarios_roles_rol`: `rol_id -> roles(id) ON UPDATE RESTRICT ON DELETE RESTRICT`.
- CHECK `ck_usuarios_roles_vigencia`: `vigente_hasta IS NULL OR vigente_hasta > vigente_desde`.
- EXCLUDE `excl_usuarios_roles_vigencia` usando GiST: `(usuario_id WITH =, rol_id WITH =, tstzrange(vigente_desde, vigente_hasta, '[)') WITH &&)`. No son legales intervalos superpuestos para el mismo usuario y rol; sí son legales intervalos consecutivos donde el nuevo inicio coincide con el fin anterior.
- Trigger de integridad `trg_usuarios_roles_misma_organizacion`, explicado en `roles`.
- Índice de acceso inmediato `idx_usuarios_roles_vigentes_por_usuario` sobre `(usuario_id, rol_id) WHERE vigente_hasta IS NULL`. No es UNIQUE: la exclusión ya garantiza la unicidad actual. Este B-tree parcial se justifica por la consulta de permisos en cada autorización.

Identidad e historial:

- La asociación necesita UUID propio porque puede haber múltiples episodios para el mismo par y el futuro sistema de auditoría podrá referirse inequívocamente al episodio afectado.
- Revocar establece una sola vez `vigente_hasta`; nunca ejecuta `DELETE`.
- Volver a otorgar después de la revocación inserta otra fila con otro UUID y otro `vigente_desde`.
- `trg_usuarios_roles_historia` rechaza `DELETE`, cambios de participantes o `vigente_desde`, reaperturas y cambios posteriores del fin. Sólo acepta `vigente_hasta: NULL -> instante` que también satisfaga el CHECK y el EXCLUDE.
- No se guardan `otorgado_por` o `revocado_por` todavía. La atribución del actor y el motivo corresponden al futuro `AuditEvent`; no son necesarios para definir la vigencia del hecho.

### 2.7. `roles_permisos`

**Propósito.** Registra cada episodio durante el cual un permiso formó parte de un rol.

| Columna | Tipo PostgreSQL | Nulabilidad / default | Semántica |
|---|---|---|---|
| `id` | `uuid` | `NOT NULL DEFAULT gen_random_uuid()` | Identidad estable del episodio. |
| `rol_id` | `uuid` | `NOT NULL` | Rol configurado. |
| `permiso_id` | `uuid` | `NOT NULL` | Permiso agregado. |
| `vigente_desde` | `timestamptz` | `NOT NULL DEFAULT now()` | Inicio inclusivo. |
| `vigente_hasta` | `timestamptz` | `NULL` | Fin exclusivo; nulo significa concesión actual. |

Claves y restricciones:

- PK: `pk_roles_permisos (id)`.
- Clave candidata semántica: `(rol_id, permiso_id, vigente_desde)`, garantizada por la exclusión sin duplicar un índice UNIQUE.
- FK `fk_roles_permisos_rol`: `rol_id -> roles(id) ON UPDATE RESTRICT ON DELETE RESTRICT`.
- FK `fk_roles_permisos_permiso`: `permiso_id -> permisos(id) ON UPDATE RESTRICT ON DELETE RESTRICT`.
- CHECK `ck_roles_permisos_vigencia`: `vigente_hasta IS NULL OR vigente_hasta > vigente_desde`.
- EXCLUDE `excl_roles_permisos_vigencia` usando GiST: `(rol_id WITH =, permiso_id WITH =, tstzrange(vigente_desde, vigente_hasta, '[)') WITH &&)`.
- Índice de acceso inmediato `idx_roles_permisos_vigentes_por_rol` sobre `(rol_id, permiso_id) WHERE vigente_hasta IS NULL`, necesario para expandir los roles vigentes del usuario a permisos vigentes.

Identidad e historial:

- La asociación tiene UUID propio por las mismas razones históricas que `usuarios_roles`.
- Retirar un permiso establece `vigente_hasta`; volver a agregarlo crea otra fila.
- `trg_roles_permisos_historia` rechaza borrados, reescritura del episodio, reapertura o una segunda modificación del fin.
- El actor y motivo se incorporarán mediante `AuditEvent` en su slice, sin ensanchar prematuramente esta relación.

## 3. Taxonomía canónica de permisos

Los códigos canónicos usan sustantivo de sección en singular y acción en infinitivo, separados por `:`. Se elige `inventario` en lugar del anglicismo `stock`, y `consola_tecnica` explicita el significado del anterior ámbito ambiguo `internal`. Los códigos no llevan acentos para ser constantes simples y estables en Rust, SQL y políticas.

| Código semántico anterior | Código canónico en español | Rótulo / significado funcional en español |
|---|---|---|
| `dashboard:view` | `panel:ver` | Ver el panel general. |
| `agriculture:view` | `agricultura:ver` | Consultar información agrícola. |
| `agriculture:create` | `agricultura:crear` | Crear registros agrícolas. |
| `agriculture:edit` | `agricultura:editar` | Modificar registros agrícolas. |
| `inventory:view` | `inventario:ver` | Consultar inventario y existencias. |
| `inventory:adjust` | `inventario:ajustar` | Registrar ajustes de inventario. |
| `livestock:view` | `ganaderia:ver` | Consultar información ganadera disponible en V1. |
| `livestock:edit` | `ganaderia:editar` | Modificar información ganadera habilitada en V1. |
| `machinery:view` | `maquinaria:ver` | Consultar maquinaria. |
| `machinery:edit` | `maquinaria:editar` | Modificar información de maquinaria. |
| `commercial:view` | `comercial:ver` | Consultar información comercial. |
| `commercial:edit` | `comercial:editar` | Modificar información comercial. |
| `configuration:view` | `configuracion:ver` | Consultar la configuración de Agro Ops. |
| `configuration:manage` | `configuracion:administrar` | Administrar la configuración de Agro Ops. |
| `internal:view` | `consola_tecnica:ver` | Consultar la consola técnica interna. |
| `internal:manage` | `consola_tecnica:administrar` | Administrar operaciones de la consola técnica interna. |

Esta tabla conserva las 16 capacidades previas. No agrega permisos propios de XRS2i ni capacidades ganaderas más finas de Etapa 7. Los rótulos y descripciones pueden evolucionar; el código sólo cambia mediante una decisión de compatibilidad y una migración explícita.

La autorización efectiva actual de un usuario es el conjunto `DISTINCT permisos.codigo` obtenido a través de:

1. organización, usuario, rol y permiso activos;
2. `usuarios_roles.vigente_hasta IS NULL`;
3. `roles_permisos.vigente_hasta IS NULL`.

La existencia de una identidad Supabase vigente permite resolver al usuario, pero no concede permisos por sí sola. Varias rutas de roles pueden producir el mismo permiso y se colapsan como conjunto. Ninguna consulta compara `roles.nombre`.

## 4. Elección de restricciones temporales

Un índice UNIQUE parcial sobre el par donde `vigente_hasta IS NULL` impediría dos filas actuales, pero no impediría insertar dos intervalos históricos cerrados que se superponen. Eso permitiría que una consulta histórica afirmara simultáneamente dos concesiones del mismo hecho y debilitaría la confiabilidad de la historia.

Por esa razón este slice **sí justifica deliberadamente** `btree_gist` y constraints `EXCLUDE` temporales:

- `btree_gist` aporta igualdad GiST para `uuid`;
- `tstzrange(..., '[)') WITH &&` representa directamente la regla de no superposición;
- PostgreSQL arbitra correctamente escrituras concurrentes, algo que un trigger de “consultar antes de insertar” no garantiza sin bloqueos adicionales;
- un extremo superior nulo se representa como infinito y por eso también impide duplicados actuales;
- el mismo mecanismo protege asignaciones de usuario, composición de rol e identidades externas simultáneas.

El costo es una extensión confiable adicional y tres índices GiST. Es aceptable porque protege una invariante histórica de autorización, no una optimización hipotética. Los dos B-tree parciales de asociaciones no duplican una regla de corrección: existen para el camino caliente conocido de permisos actuales y no son UNIQUE.

## 5. Semántica no destructiva consolidada

| Relación | Actualizaciones admitidas | Retiro / revocación | Borrado |
|---|---|---|---|
| `organizaciones` | `nombre`, `activa` | `activa = false` | No expuesto; FK `RESTRICT` si fue referenciada. |
| `usuarios` | `nombre_completo`, `activo` | `activo = false` | No expuesto; FK `RESTRICT`; organización inmutable. |
| `roles` | `nombre`, `descripcion`, `activo` | `activo = false` | No expuesto; FK `RESTRICT`; organización inmutable. |
| `permisos` | `nombre`, `descripcion`, `activo` | `activo = false` | No expuesto; FK `RESTRICT`; código inmutable. |
| `usuarios_roles` | Sólo cerrar una vez `vigente_hasta` | Cerrar intervalo; una nueva alta es otra fila | Rechazado por trigger específico. |
| `roles_permisos` | Sólo cerrar una vez `vigente_hasta` | Cerrar intervalo; una nueva alta es otra fila | Rechazado por trigger específico. |
| `identidades_autenticacion_externas` | Sólo fijar una vez `desvinculada_en` | Cerrar vínculo; otro sujeto es otra fila | Rechazado por trigger específico. |

No se aplica un trigger universal de borrado a toda tabla. Los triggers anti-borrado se reservan para las tres relaciones cuya fila es en sí misma un hecho histórico. En los maestros, el runtime usa desactivación y los FK `RESTRICT` impiden borrar cualquier maestro que sostenga historia. Esto permite que una migración privilegiada corrija una fila maestra jamás utilizada sin fingir que toda fila de configuración es ya un evento histórico.

Los CHECK y EXCLUDE validan la forma e integridad temporal; los triggers históricos validan transiciones entre el valor anterior y el nuevo, algo que un CHECK de fila no puede expresar. El futuro `AuditEvent` agregará actor, motivo, instante de comando y valores relevantes de los cambios. No reemplazará estas restricciones ni será necesario para reconstruir si una asignación estaba vigente.

## 6. Revisión de normalización

### `organizaciones`

- Clave candidata: `id`.
- FD importantes: `id -> nombre, activa, creada_en`.
- Forma normal más alta justificada: BCNF. Todo determinante no trivial es clave candidata.
- 4NF: satisfecha; no hay dependencias multivaluadas independientes dentro de la fila.
- 5NF: satisfecha de manera trivial; no hay dependencia de join no trivial demostrada.

### `usuarios`

- Clave candidata: `id`.
- FD importantes: `id -> organizacion_id, nombre_completo, activo, creado_en`.
- Forma normal más alta justificada: BCNF. La pertenencia a organización es un único hecho del usuario.
- 4NF: satisfecha. Roles e identidades, que son conjuntos independientes potenciales, están en relaciones separadas.
- 5NF: satisfecha de manera trivial; no hay dependencia de join no trivial.

### `identidades_autenticacion_externas`

- Claves candidatas: `id`; `(proveedor, sujeto_proveedor)`; y, para identificar un episodio de un usuario, `(usuario_id, vinculada_en)` bajo la exclusión temporal.
- FD importantes: cada clave candidata determina `usuario_id`, namespace/sujeto y ambos extremos del vínculo. En particular, `(proveedor, sujeto_proveedor) -> usuario_id, vinculada_en, desvinculada_en`.
- Forma normal más alta justificada: BCNF. No se copian email, organización ni claims determinados por otras entidades.
- 4NF: satisfecha. Cada fila es un vínculo indivisible; posibles vínculos sucesivos son filas, no columnas repetidas.
- 5NF: no hay dependencia de join no trivial demostrada; la relación no se descompone sin perder el hecho de vinculación temporal.

### `roles`

- Claves candidatas: `id`; `(organizacion_id, nombre)`.
- FD importantes: `id -> organizacion_id, nombre, descripcion, activo, creado_en` y `(organizacion_id, nombre) -> id, descripcion, activo, creado_en`.
- Forma normal más alta justificada: BCNF; ambos determinantes son claves candidatas.
- 4NF: satisfecha. Usuarios asignados y permisos agrupados son multivalores independientes y están separados en `usuarios_roles` y `roles_permisos`.
- 5NF: no hay dependencia de join no trivial adicional.

### `permisos`

- Claves candidatas: `id`; `codigo`.
- FD importantes: `id -> codigo, nombre, descripcion, activo, creado_en` y `codigo -> id, nombre, descripcion, activo, creado_en`.
- Forma normal más alta justificada: BCNF. No se almacenan `seccion` y `accion` como hechos derivados adicionales.
- 4NF: satisfecha; una fila describe una capacidad atómica.
- 5NF: satisfecha de manera trivial.

### `usuarios_roles`

- Claves candidatas: `id`; `(usuario_id, rol_id, vigente_desde)` bajo la exclusión.
- FD importantes: `id -> usuario_id, rol_id, vigente_desde, vigente_hasta` y `(usuario_id, rol_id, vigente_desde) -> id, vigente_hasta`.
- Forma normal más alta justificada: BCNF. No se duplica `organizacion_id`; su coherencia se valida contra maestros mediante trigger.
- 4NF: satisfecha. Cada fila representa un solo episodio usuario–rol; los distintos roles y períodos son filas independientes.
- 5NF: no existe una dependencia de join no trivial demostrada. Separar usuario, rol y tiempo perdería la asociación ternaria del episodio.

### `roles_permisos`

- Claves candidatas: `id`; `(rol_id, permiso_id, vigente_desde)` bajo la exclusión.
- FD importantes: `id -> rol_id, permiso_id, vigente_desde, vigente_hasta` y `(rol_id, permiso_id, vigente_desde) -> id, vigente_hasta`.
- Forma normal más alta justificada: BCNF.
- 4NF: satisfecha. Cada concesión temporal es un hecho atómico y no mezcla el conjunto independiente de usuarios del rol.
- 5NF: no existe una dependencia de join no trivial demostrada.

El esquema alcanza al menos 3NF y, relación por relación, BCNF. La separación de identidades externas, roles de usuario y permisos de rol también evita dependencias multivaluadas genuinas, por lo que alcanza 4NF. No se postulan descomposiciones 5NF sin una dependencia de join real. No se hace ninguna afirmación sobre “cumplir las 12 reglas de Codd”; el diseño simplemente preserva identidad, dominios atómicos, integridad referencial y hechos normalizados dentro de PostgreSQL.

## 7. Bootstrap y datos canónicos

### Permisos

La migración de Slice 2.1 debe sembrar exactamente los 16 permisos de la tabla de equivalencias. Son vocabulario canónico del producto y deben existir igual en local, staging y producción. El `id` puede generarse con su default UUID durante la migración: el backend usa `codigo` como identidad semántica y los FK internos resuelven los UUID dentro de cada base. No se codifican UUID de permisos en Rust.

La siembra debe ser parte de la misma migración, después de crear restricciones y triggers, mediante un `INSERT` explícito de código, nombre y descripción. Como una migración SQLx se ejecuta una sola vez y `codigo` es UNIQUE, no se requiere un `ON CONFLICT` que oculte diferencias.

### Organización y roles

- La organización no se siembra en una migración: su nombre es configuración de cada instalación.
- Tampoco se siembran roles, porque pertenecen a la organización y son configuración administrable. No hay significado de autorización asociado al nombre “Administrador” u otro rol inicial.
- Un comando de bootstrap controlado y restringido operacionalmente crea, en una única transacción, la organización, los roles iniciales, sus `roles_permisos`, el primer usuario, su identidad externa y su `usuarios_roles`.
- Ese bootstrap debe ser repetible por estado observado o por un identificador de operación cuando exista la primitiva de idempotencia de su slice; la migración no debe fingir una idempotencia todavía inexistente.

### Usuario de Supabase ya existente en staging

El operador obtiene el UUID estable del usuario de staging desde Supabase Auth (`auth.users.id`, equivalente al `sub` verificado) y lo entrega como entrada explícita al bootstrap controlado. La transacción crea `usuarios` y `identidades_autenticacion_externas` con:

- `proveedor = 'supabase'`;
- `sujeto_proveedor = <UUID estable provisto>`;
- el `usuario_id` interno recién creado o previamente seleccionado de manera explícita.

No se busca ni vincula por email, y no se incluyen IDs de usuarios, emails, contraseñas, URLs de proyecto o secretos específicos de ambiente en la migración. El mismo procedimiento sirve para local, staging y producción con entradas propias de cada entorno.

## 8. Orden exacto para la próxima migración

La implementación SQLx/PostgreSQL siguiente debe respetar este orden, sin crear otras relaciones:

1. Ejecutar `CREATE EXTENSION IF NOT EXISTS btree_gist`; es necesaria antes de declarar igualdad GiST de UUID en los EXCLUDE. La migración previa de PostGIS permanece intacta.
2. Crear `organizaciones` con PK y CHECK inline.
3. Crear `permisos` con PK, `uq_permisos_codigo` y CHECK inline.
4. Crear `usuarios` con PK, FK a `organizaciones` y CHECK inline.
5. Crear `roles` con PK, FK a `organizaciones`, `uq_roles_organizacion_nombre` y CHECK inline.
6. Crear `identidades_autenticacion_externas` con PK, FK a `usuarios`, `uq_identidades_auth_proveedor_sujeto` y CHECK inline.
7. Agregar `excl_identidades_auth_usuario_vigencia`. Se agrega después de crear la tabla y con `btree_gist` ya disponible.
8. Crear `usuarios_roles` con PK, ambos FK y el CHECK temporal inline.
9. Agregar `excl_usuarios_roles_vigencia`.
10. Crear `roles_permisos` con PK, ambos FK y el CHECK temporal inline.
11. Agregar `excl_roles_permisos_vigencia`.
12. Crear `idx_usuarios_roles_vigentes_por_usuario`.
13. Crear `idx_roles_permisos_vigentes_por_rol`.
14. Crear las funciones y triggers de invariantes maestras: organización inmutable de `usuarios` y `roles`, y código inmutable de `permisos`.
15. Crear la función y `trg_usuarios_roles_misma_organizacion`; requiere que `usuarios`, `roles` y `usuarios_roles` ya existan.
16. Crear las funciones y triggers históricos específicos para `identidades_autenticacion_externas`, `usuarios_roles` y `roles_permisos`. Deben rechazar `DELETE` y permitir como única actualización histórica el cierre terminal del intervalo correspondiente.
17. Insertar las 16 filas canónicas de `permisos`, con los códigos y rótulos definidos en este documento.

PK y UNIQUE crean sus propios índices B-tree; no deben duplicarse. Los EXCLUDE crean los índices GiST que necesitan. Los únicos índices explícitos adicionales son los dos B-tree parciales del camino conocido de autorización actual.

## 9. Resultado y límites

El diseño queda cerrado para implementar la migración sin decidir nuevamente nombres, tipos, claves, vigencias, alcance de roles, identidad Supabase, taxonomía, índices, extensiones o estrategia de bootstrap. No quedan preguntas arquitectónicas abiertas para Slice 2.1.

Quedan deliberadamente para tareas posteriores:

- la migración SQL y su validación real en PostgreSQL/PostGIS;
- casos de uso y middleware de autorización en Rust;
- UI de administración;
- `AuditEvent` y atribución de actores/motivos;
- idempotencia, jobs, reintentos y outbox;
- documentos, Storage y referencias externas de negocio;
- entidades de agricultura, inventario, ganadería, maquinaria y comercial;
- ARCA, Finnegans y XRS2i.
