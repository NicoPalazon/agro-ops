# Etapa 2, Slice 2.2: diseño de autorización efectiva en el backend

## 1. Alcance y decisión central

Este slice agrega la mínima primitiva reutilizable para que el backend de Agro Ops convierta una identidad ya autenticada por Supabase en un actor interno y autorice por permisos canónicos. No cambia la autenticación de Etapa 1 ni el esquema físico de Slice 2.1.

El flujo aprobado es:

```text
Bearer token
    -> Supabase Auth valida la sesión y devuelve su id estable
    -> identidades_autenticacion_externas resuelve el Usuario interno
    -> usuarios y organizaciones validan que el actor esté habilitado
    -> usuarios_roles, roles_permisos, roles y permisos producen permisos efectivos
    -> Rust exige un código canónico para ejecutar el handler
```

Decisiones rectoras:

- Supabase Auth sigue siendo dueño de autenticación y sesiones.
- PostgreSQL sigue siendo dueño del estado de autorización.
- Rust es el límite que exige permisos antes de ejecutar el caso de uso o handler protegido.
- Los roles sólo agrupan permisos. Ninguna decisión compara `roles.nombre`.
- El sujeto externo se resuelve por `('supabase', sujeto_proveedor)`; nunca por email.
- Se construye un contexto con todos los permisos efectivos una vez por request protegido y en una sola consulta SQL.
- El contexto vive sólo durante el request. No hay caché entre requests, tabla derivada, Redis ni claims de autorización incorporados al JWT.
- La primera superficie protegida son exactamente `GET /internal/worker/status` y `GET /internal/system-status`, ambas con `consola_tecnica:ver`.
- `GET /health`, `GET /ready` y `GET /version` permanecen públicos. `GET /openapi.json` conserva su autenticación actual y no se incorpora a la prueba de autorización de este slice.

## 2. Autenticación existente que se reutiliza

La implementación actual está en `services/backend/src/auth.rs`, `services/backend/src/lib.rs` y `services/backend/src/bin/api.rs`.

### 2.1. Extracción del bearer token

`require_authenticated_user`, hoy en `services/backend/src/lib.rs`, recibe el `HeaderMap` del handler y:

1. busca `Authorization`;
2. exige que sea texto válido;
3. exige el prefijo existente y exacto `Bearer `;
4. rechaza un valor vacío;
5. entrega el token a `AppState.auth.verify(...)`.

La ausencia del header, un header no textual, un esquema distinto, un token vacío o un token rechazado producen hoy `401 Unauthorized`. Slice 2.2 debe conservar esa extracción y esas reglas; la función se refactoriza para devolver `auth::AuthenticatedUser` en vez de descartarlo.

### 2.2. Establecimiento de validez y sujeto autenticado

`auth::AccessTokenVerifier` es el puerto existente. Su implementación de producción, `auth::SupabaseAuthVerifier`, valida el access token consultando `GET <SUPABASE_URL>/auth/v1/user` con:

- el access token como bearer;
- la publishable key en `apikey`;
- timeout de cinco segundos.

No se decodifica el JWT localmente en el handler. Supabase valida la sesión y devuelve su usuario. Una respuesta `401` o `403` de Supabase se traduce a `VerifyAccessTokenError::Invalid`; fallas de red, timeout y respuestas no exitosas distintas se traducen a `VerifyAccessTokenError::Unavailable`.

`SupabaseUserResponse.id` es el identificador estable de Supabase Auth, equivalente al `auth.users.id` y al `sub` autenticado. El verifier lo expone como `auth::AuthenticatedUser.id: String`. Éste es el valor que Slice 2.2 debe convertir a `uuid` y usar como `sujeto_proveedor`; no debe extraer email, consultar `auth.users` ni volver a validar el token.

Se reutilizan sin reemplazo:

- `auth::AuthenticatedUser`;
- `auth::AccessTokenVerifier`;
- `auth::SupabaseAuthVerifier`;
- `auth::VerifyAccessTokenError::{Invalid, Unavailable}`;
- `AppState.auth` y la construcción de producción de `src/bin/api.rs`;
- la extracción centralizada del header que hoy realiza `require_authenticated_user`;
- `TestAccessTokenVerifier` como patrón de prueba de la frontera HTTP.

El `String` autenticado se parsea una vez con `Uuid::parse_str` antes de consultar PostgreSQL. Un identificador no UUID en una respuesta supuestamente válida de Supabase no puede representar el sujeto previsto por el esquema y se trata como autenticación inválida (`401`), sin enviar ese valor a SQL.

## 3. Separación entre autenticación y autorización

La autenticación sólo responde “Supabase reconoce este access token y este es su sujeto estable”. La autorización responde “ese sujeto tiene acceso interno a Agro Ops y posee el permiso requerido ahora”.

La función HTTP existente se divide conceptualmente en dos operaciones:

```rust
async fn authenticate_request(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<auth::AuthenticatedUser, RequestAccessError>;

async fn authorize_request(
    state: &AppState,
    headers: &HeaderMap,
    required_permission: &'static str,
) -> Result<authorization::AuthorizationContext, RequestAccessError>;
```

`authenticate_request` conserva la extracción y usa el verifier existente. `authorize_request` la llama una sola vez, convierte el sujeto a UUID, resuelve el contexto en PostgreSQL y exige el permiso. Los handlers no leen JWT, no conocen tablas y no comparan roles.

`GET /openapi.json` continúa llamando sólo a `authenticate_request`. Los dos handlers `/internal/...` llaman a `authorize_request` con la constante de `consola_tecnica:ver`.

## 4. Resolución de identidad interna

La clave del proveedor es la representación canónica ya aprobada:

```rust
pub const SUPABASE_PROVIDER: &str = "supabase";
```

La resolución exige simultáneamente:

- `identidades_autenticacion_externas.proveedor = 'supabase'`;
- `sujeto_proveedor` igual al UUID autenticado;
- el instante de autorización dentro de `[vinculada_en, desvinculada_en)`;
- `usuarios.activo = true`;
- `organizaciones.activa = true`.

La FK de Slice 2.1 enlaza identidad con usuario y usuario con organización. No se consulta Supabase nuevamente, no hay FK a `auth.users` y no participa el email.

Resultados:

| Estado interno | Resultado interno | HTTP |
|---|---|---|
| No existe vínculo vigente para el sujeto | `PrincipalUnavailable` | `403` |
| El vínculo existe pero terminó o aún no comenzó | `PrincipalUnavailable` | `403` |
| El usuario está inactivo | `PrincipalUnavailable` | `403` |
| La organización está inactiva | `PrincipalUnavailable` | `403` |
| Hay exactamente un actor activo | Se construye el contexto | continúa |
| Aparecen dos actores para el mismo proveedor/sujeto | `InvariantViolation` | `500` |
| PostgreSQL no puede resolver el actor | `DatabaseUnavailable` | `503` |

Los primeros cuatro casos se colapsan deliberadamente en la misma categoría y respuesta pública. El cliente sólo conoce que una identidad autenticada no tiene acceso; no descubre si existe un usuario, vínculo u organización concretos. La cardinalidad ambigua es imposible mientras estén vigentes `uq_identidades_auth_proveedor_sujeto` y los FK, pero Rust debe verificar que la consulta devuelva cero o una fila y fallar con `500` si la invariante se rompe.

## 5. Permisos efectivos y noción de tiempo

### 5.1. Instante de autorización

Cada resolución captura una única vez `statement_timestamp()` en PostgreSQL. Ese valor representa el inicio de la sentencia de autorización y se reutiliza para el vínculo externo, `usuarios_roles` y `roles_permisos`.

No se usa el reloj del proceso Rust ni `clock_timestamp()`. El contexto refleja el estado visible por PostgreSQL al inicio de esa consulta. Una revocación posterior afecta al request siguiente; no se conserva el contexto fuera del request actual.

Para cada relación temporal se aplica exactamente:

```sql
tstzrange(inicio, fin, '[)') @> instante
```

Por lo tanto el comienzo es inclusivo, el fin exclusivo, un comienzo futuro no autoriza y un fin nulo representa vigencia sin límite superior.

### 5.2. Camino efectivo

Un código es efectivo sólo si existe el siguiente camino completo en el instante capturado:

```text
Usuario activo
  -> Organización activa
  -> UsuarioRol vigente
  -> Rol activo
  -> RolPermiso vigente
  -> Permiso activo
  -> permisos.codigo
```

Se excluyen explícitamente usuarios, organizaciones, roles y permisos inactivos. `roles.nombre`, `roles.descripcion`, nombres humanos y emails no intervienen. Los códigos se deduplican: recibir el mismo permiso por dos roles tiene el mismo efecto booleano que recibirlo por uno.

### 5.3. Consulta aprobada

La implementación debe usar una sola consulta parametrizada equivalente a ésta:

```sql
WITH instante AS MATERIALIZED (
    SELECT statement_timestamp() AS ahora
),
actor AS (
    SELECT
        identidad.id AS identidad_id,
        usuario.id AS usuario_id,
        organizacion.id AS organizacion_id,
        instante.ahora AS autorizado_en
    FROM instante
    JOIN public.identidades_autenticacion_externas AS identidad
      ON tstzrange(
             identidad.vinculada_en,
             identidad.desvinculada_en,
             '[)'
         ) @> instante.ahora
    JOIN public.usuarios AS usuario
      ON usuario.id = identidad.usuario_id
     AND usuario.activo
    JOIN public.organizaciones AS organizacion
      ON organizacion.id = usuario.organizacion_id
     AND organizacion.activa
    WHERE identidad.proveedor = $1
      AND identidad.sujeto_proveedor = $2
)
SELECT
    actor.identidad_id,
    actor.usuario_id,
    actor.organizacion_id,
    COALESCE(permisos.codigos, ARRAY[]::text[]) AS codigos_permisos
FROM actor
LEFT JOIN LATERAL (
    SELECT array_agg(DISTINCT permiso.codigo ORDER BY permiso.codigo) AS codigos
    FROM public.usuarios_roles AS usuario_rol
    JOIN public.roles AS rol
      ON rol.id = usuario_rol.rol_id
     AND rol.organizacion_id = actor.organizacion_id
     AND rol.activo
    JOIN public.roles_permisos AS rol_permiso
      ON rol_permiso.rol_id = rol.id
     AND tstzrange(
             rol_permiso.vigente_desde,
             rol_permiso.vigente_hasta,
             '[)'
         ) @> actor.autorizado_en
    JOIN public.permisos AS permiso
      ON permiso.id = rol_permiso.permiso_id
     AND permiso.activo
    WHERE usuario_rol.usuario_id = actor.usuario_id
      AND tstzrange(
              usuario_rol.vigente_desde,
              usuario_rol.vigente_hasta,
              '[)'
          ) @> actor.autorizado_en
) AS permisos ON TRUE;
```

Parámetros:

1. `$1 = authorization::SUPABASE_PROVIDER`;
2. `$2 = sujeto Supabase` ya parseado como `Uuid`.

La consulta devuelve una fila incluso si el actor no tiene permisos, con `codigos_permisos = {}`. Debe ejecutarse con parámetros enlazados de SQLx. Se usa `fetch_all`, no `fetch_optional`, para comprobar explícitamente cardinalidad cero, uno o más de uno. `identidad_id` sólo permite detectar filas externas duplicadas; no forma parte del contexto entregado al handler.

## 6. Estrategia de consulta seleccionada

Se selecciona “resolver actor y todos los permisos efectivos una vez por request”.

Ventajas para V1:

- una sola ida a PostgreSQL por request protegido;
- un único instante de autorización para identidad, asignaciones y concesiones;
- los handlers reciben una abstracción estable y no conocen SQL ni roles;
- varios checks dentro del mismo request no repiten autenticación o consultas;
- el contexto ya lleva `usuario_id` y `organizacion_id` para futura atribución de `AuditEvent`, sin implementar auditoría ahora;
- una colección máxima de 16 códigos actuales es pequeña y simple de probar;
- cada request vuelve a PostgreSQL, por lo que desactivaciones y revocaciones no quedan ocultas por una caché.

No se elige una consulta de existencia por permiso porque separaría resolución de actor y autorización, agregaría viajes si un endpoint exige más de una capacidad y dificultaría reutilizar al actor. Tampoco se persisten permisos efectivos ni se agregan caches.

## 7. Tipos Rust mínimos

En `services/backend/src/authorization.rs`:

```rust
use std::collections::BTreeSet;
use uuid::Uuid;

pub const SUPABASE_PROVIDER: &str = "supabase";

pub mod permission_codes {
    pub const CONSOLA_TECNICA_VER: &str = "consola_tecnica:ver";
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthorizationContext {
    pub user_id: Uuid,
    pub organization_id: Uuid,
    permission_codes: BTreeSet<String>,
}

impl AuthorizationContext {
    pub fn permission_codes(&self) -> &BTreeSet<String>;
    pub fn has_permission(&self, required: &str) -> bool;
    pub fn require_permission(&self, required: &str) -> Result<(), PermissionDenied>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PermissionDenied;

#[derive(Debug)]
pub enum ResolveAuthorizationError {
    PrincipalUnavailable,
    InvariantViolation,
    DatabaseUnavailable(sqlx::Error),
}

pub async fn resolve_context(
    db: &sqlx::PgPool,
    supabase_subject: Uuid,
) -> Result<AuthorizationContext, ResolveAuthorizationError>;
```

Los nombres Rust siguen el estilo inglés del backend actual; los nombres físicos y los códigos funcionales permanecen en español. `BTreeSet` expresa semántica de conjunto y da orden determinista en pruebas. No se expone una lista de roles.

Sólo se crea la constante de permiso consumida en este slice. Los demás códigos se agregan como constantes cuando un caso de uso real los exija; no se duplica prematuramente todo el catálogo. Los rótulos nunca se convierten en constantes de autorización.

Para decodificar UUID directamente desde SQLx, la implementación debe agregar `uuid` a las features ya declaradas de `sqlx` en `services/backend/Cargo.toml`. No se necesita `chrono`, macros SQLx ni otra dependencia.

## 8. Errores y frontera HTTP

`services/backend/src/lib.rs` debe definir una categoría HTTP privada, separada de los errores de repositorio:

```rust
enum RequestAccessError {
    AuthenticationFailed,
    AuthenticationUnavailable,
    PrincipalDenied,
    PermissionDenied,
    AuthorizationDatabaseUnavailable,
    AuthorizationInvariantViolation,
}
```

Mapeo obligatorio:

| Categoría | HTTP | Significado público |
|---|---:|---|
| Falta bearer, bearer malformado, expirado o rechazado | `401` | No se estableció autenticación válida |
| Supabase no está disponible para validar | `503` | Dependencia de autenticación no disponible |
| Sujeto válido sin actor interno vigente/habilitado | `403` | Actor autenticado sin acceso |
| Actor interno sin el permiso requerido | `403` | Actor autenticado sin acceso |
| PostgreSQL no está disponible para autorizar | `503` | Dependencia de autorización no disponible |
| Cardinalidad o dato imposible según restricciones | `500` | Error interno de integridad |

Los `401`, ambos `403` y los `5xx` devuelven únicamente el status y un cuerpo vacío, preservando el estilo mínimo actual. En particular, ambos `403` son indistinguibles para el cliente. Nunca se envían IDs, existencia de roles/permisos, códigos efectivos, SQL ni detalles de Supabase.

`VerifyAccessTokenError::Invalid` mapea a `AuthenticationFailed`; `Unavailable` mapea a `AuthenticationUnavailable`. Esto corrige la ambigüedad actual que transforma indisponibilidad de Supabase en `401`, sin cambiar el verifier.

Los errores de SQLx se registran con `tracing` en el límite HTTP y luego se convierten a `503`. La violación de cardinalidad se registra como error de invariante y se convierte a `500`. Ningún error de infraestructura se presenta como `403` y ningún fallo autoriza por defecto.

El middleware `request_tracing` actual sigue envolviendo toda respuesta. Por eso también añade `x-request-id` y `x-correlation-id`, registra el status y completa el span para `401`, `403`, `500` y `503`. Los logs no deben incluir access tokens, emails ni el conjunto de permisos. Pueden incluir la categoría interna, el permiso requerido y, cuando ya se resolvió, el UUID interno del usuario para diagnóstico estructurado.

## 9. Ubicación y dependencias

La implementación siguiente debe limitarse a:

| Archivo | Responsabilidad |
|---|---|
| `services/backend/src/auth.rs` | Reutilizar `AuthenticatedUser`, el trait y el verifier; no incorporar SQL ni permisos |
| `services/backend/src/authorization.rs` (nuevo) | Constante de proveedor, código requerido, contexto, check, consulta PostgreSQL y errores independientes de Axum |
| `services/backend/src/lib.rs` | Exportar `authorization`, extraer/autenticar, resolver el contexto, mapear 401/403/5xx y proteger los dos handlers Internal |
| `services/backend/src/bin/api.rs` | Sin nueva dependencia de autorización; continúa construyendo `AppState` con `PgPool` y verifier |
| `services/backend/Cargo.toml` | Agregar únicamente la feature SQLx `uuid` |
| `services/backend/tests/authorization_api_integration.rs` (nuevo) | Fixtures SQL, resolución real y pruebas HTTP enfocadas contra PostgreSQL |

No se agrega otro crate, trait genérico de RBAC, macro, middleware global de permisos ni repositorio de roles visible para handlers. La consulta queda encapsulada en `authorization`; la frontera Axum sólo conoce el contexto y los errores tipados.

## 10. Primera superficie protegida

Slice 2.2 modifica exactamente:

- `GET /internal/worker/status`;
- `GET /internal/system-status`.

Cada handler empieza con el equivalente a:

```rust
let authorization = authorize_request(
    &state,
    &headers,
    authorization::permission_codes::CONSOLA_TECNICA_VER,
)
.await?;
```

Aunque el handler todavía no consuma los IDs, conserva `authorization` como actor disponible. El permiso es exactamente `consola_tecnica:ver`.

Los metadatos OpenAPI de ambos endpoints deben agregar respuestas `403` y `503` además del `401` existente. La seguridad bearer sigue documentada igual. Los endpoints públicos `/health`, `/ready` y `/version` no llaman autenticación ni autorización.

Como la autorización depende de PostgreSQL, `/internal/system-status` ya no puede devolver `200` con `database = not_ready` cuando la base está caída: debe fallar cerrado con `503` antes del handler. El test existente de ese escenario debe actualizarse. Esto no afecta `/ready`, que continúa siendo la señal pública de disponibilidad de base.

## 11. Semántica API completa

| Caso | Resultado |
|---|---|
| A. Sin bearer token | `401` |
| B. Bearer malformado, inválido o expirado | `401` |
| C. Token válido sin vínculo interno vigente | `403` |
| D. Vínculo válido con `Usuario.activo = false` | `403` |
| D2. Usuario válido con `Organizacion.activa = false` | `403` |
| E. Actor válido sin `consola_tecnica:ver` | `403` |
| F. El permiso llega por un rol vigente y activo | el handler responde normalmente |
| G. `usuarios_roles` expirado | `403` si no existe otro camino vigente |
| H. `usuarios_roles` comienza en el futuro | `403` si no existe otro camino vigente |
| I. `roles_permisos` expirado | `403` si no existe otro camino vigente |
| I2. `roles_permisos` comienza en el futuro | `403` si no existe otro camino vigente |
| J. Rol activo sin el permiso requerido | `403` |
| K. Rol llamado “Administrador” sin permiso requerido | `403` |
| L. El permiso llega por varios roles | presente una sola vez y autoriza |
| M. Rol inactivo o permiso inactivo | ese camino no autoriza |
| N. Supabase no responde | `503` |
| O. PostgreSQL no responde durante autorización | `503` |
| P. Resolución ambigua contraria a constraints | `500` |

El status normal del handler conserva su significado actual: por ejemplo, un actor autorizado puede recibir el `503` propio de `/internal/worker/status` si la lectura posterior del heartbeat falla. Ese error ocurre después de autorizar y no se confunde con falta de permiso.

## 12. Fixtures y estrategia de pruebas

### 12.1. Autenticación

Se conservan las pruebas de Etapa 1 en `auth.rs` que demuestran envío del bearer/publishable key, rechazo de `401/403`, payload inválido, timeout y error remoto. No se duplican en Slice 2.2.

Las pruebas de router existentes siguen demostrando bearer ausente, malformado e inválido. Debe agregarse un verifier de prueba que devuelva `VerifyAccessTokenError::Unavailable` para comprobar `503`, porque antes ambos errores se colapsaban en `401`.

### 12.2. Datos SQL deterministas

Los tests crean mediante SQL normal, con UUID aleatorios de prueba y nombres únicos:

1. `organizaciones`;
2. `usuarios`;
3. `identidades_autenticacion_externas` con `proveedor = 'supabase'` y un sujeto UUID ficticio;
4. `roles`;
5. `usuarios_roles`;
6. el `permisos.id` existente para `consola_tecnica:ver`;
7. `roles_permisos`.

Los tiempos se expresan en relación con el reloj de PostgreSQL al insertar: pasado para períodos expirados, futuro para los aún no iniciados y nulo para el extremo actual. No se usa un usuario, email, UUID o secreto real de staging. No se agregan seeds ni migraciones.

Como las relaciones históricas prohíben borrado, la suite debe ejecutarse contra la base PostgreSQL/PostGIS descartable usada por el gate y generar identificadores únicos. Los fixtures se insertan mediante el `PgPool` y permanecen sólo en esa base descartable; no se intenta limpiar con `DELETE` o `TRUNCATE` ni se generaliza la API del resolver sólo por testing.

### 12.3. Pruebas directas de autorización PostgreSQL

En `tests/authorization_api_integration.rs`, contra PostgreSQL real:

- vínculo vigente + usuario/organización activos resuelve IDs internos correctos;
- falta de vínculo, vínculo expirado y vínculo futuro producen `PrincipalUnavailable`;
- usuario inactivo y organización inactiva producen `PrincipalUnavailable`;
- rol y grant vigentes producen el código;
- `usuarios_roles` expirado y futuro no producen el código;
- `roles_permisos` expirado y futuro no producen el código;
- rol inactivo y permiso inactivo no producen el código;
- un rol llamado `Administrador` sin grant no autoriza;
- dos roles vigentes con el mismo permiso producen un solo código;
- una falla de conexión produce `DatabaseUnavailable`, no `PermissionDenied`.

Las variantes temporales pueden compartir una tabla de casos si cada una construye intervalos inequívocos alrededor del tiempo de base.

### 12.4. Pruebas HTTP

Usando `app`, el verifier falso existente y fixtures PostgreSQL:

- sin bearer -> `401`;
- bearer inválido -> `401`;
- verifier no disponible -> `503`;
- autenticado pero no aprovisionado -> `403`;
- usuario inactivo -> `403`;
- actor sin permiso -> `403`;
- actor con `consola_tecnica:ver` -> éxito en ambos endpoints seleccionados;
- `UsuarioRol` expirado -> `403`;
- `RolPermiso` expirado -> `403`;
- rol `Administrador` sin grant -> `403`;
- PostgreSQL no disponible para autorización -> `503`;
- los `401`, `403` y `503` incluyen `x-request-id` y `x-correlation-id`;
- las respuestas `403` de no aprovisionado y permiso ausente son públicamente iguales.

No se necesita navegador ni frontend E2E.

## 13. Seguridad

- Toda entrada variable de la consulta (`proveedor` y sujeto UUID) se enlaza con parámetros SQLx; no se concatena SQL.
- El token nunca llega a PostgreSQL y nunca se registra.
- El email no se solicita ni almacena para autorizar.
- Los handlers sólo exigen constantes de códigos canónicos; no aceptan el permiso requerido desde el request.
- `consola_tecnica:ver` vive en una constante Rust; nombres o descripciones mutables no afectan semántica.
- No hay API que autorice por nombre de rol.
- La consulta vuelve a evaluar estado y vigencia en cada request, evitando permisos obsoletos por caché.
- Usuario, organización, rol y permiso inactivos se excluyen de forma cerrada.
- Un error de Supabase o PostgreSQL niega la ejecución del handler y devuelve `5xx`, nunca acceso ni un falso `403`.
- Los `403` no distinguen vínculo ausente, cuenta deshabilitada o permiso faltante.
- Los detalles de SQLx y las identidades internas quedan sólo en logs estructurados; no se exponen al cliente.
- El middleware actual conserva request/correlation IDs y status en todas las salidas de autorización.

## 14. Orden de implementación para el siguiente task

1. Agregar la feature SQLx `uuid` en `services/backend/Cargo.toml`.
2. Crear `services/backend/src/authorization.rs` con constantes, tipos, check y consulta exacta.
3. Exportar el módulo desde `services/backend/src/lib.rs`.
4. Refactorizar la extracción actual como `authenticate_request` que devuelve `AuthenticatedUser` y distingue `Invalid` de `Unavailable`.
5. Agregar `authorize_request`, parseo UUID, resolución, check y mapeo HTTP cerrado.
6. Aplicar `consola_tecnica:ver` a los dos handlers Internal seleccionados.
7. Actualizar sus respuestas OpenAPI `401/403/503` sin alterar rutas públicas.
8. Agregar fixtures y pruebas PostgreSQL/API de Slice 2.2.
9. Actualizar únicamente los tests existentes cuyos supuestos cambian, especialmente System Status con base no disponible.

No se requiere migración ni cambio de esquema.

## 15. Fuera de alcance y estado de cierre

Este diseño no introduce administración de usuarios/roles/permisos, bootstrap final, signup, frontend, auditoría, autoría, idempotencia, jobs, retries, outbox, documentos, ARCA, Finnegans, XRS2i, entidades ganaderas, permisos de otros módulos, ownership por fila, ACL, RLS, multi-tenancy SaaS, Redis o caché.

El usuario real de staging deberá vincularse en un task posterior de aprovisionamiento controlado usando su UUID estable de Supabase como entrada operacional. No se incorpora ese UUID ni ningún secreto al código o a una migración.

No quedan decisiones arquitectónicas abiertas para implementar Slice 2.2.
