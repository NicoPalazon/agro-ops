# ADR-005: Supabase para DB, Auth y Storage

## Estado

Aceptado

## Contexto

Agro Ops necesita en staging y producción:

- PostgreSQL;
- PostGIS;
- autenticación de usuarios;
- almacenamiento privado de archivos;
- administración de infraestructura;
- separación clara entre ambientes.

Mantener todos estos componentes manualmente aumentaría la carga operativa inicial del proyecto.

El sistema tendrá aproximadamente 10 usuarios en su primera etapa, por lo que se prioriza simplicidad operativa y confiabilidad.

## Decisión

Supabase será utilizado en staging y producción para:

- PostgreSQL administrado;
- PostGIS;
- Supabase Auth;
- Supabase Storage.

Agro Ops seguirá utilizando PostgreSQL como base de datos real.

Supabase funciona como proveedor de infraestructura y servicios alrededor de PostgreSQL.

## Base de datos

La aplicación se conectará a PostgreSQL mediante SQLx.

El backend Rust será responsable de las reglas de negocio y de la persistencia autorizada.

No se utilizará Supabase como sustituto de la capa de dominio o aplicación.

## Autenticación

Supabase Auth será responsable de:

- identidad del usuario;
- login;
- sesión;
- emisión y validación de tokens según el mecanismo implementado.

La autorización efectiva será responsabilidad del backend Rust.

El hecho de que un usuario esté autenticado no significa que tenga permiso para ejecutar cualquier operación.

Ejemplo:

    Supabase Auth:
    "este usuario es Nico"

    Backend Rust:
    inventory.read   = permitido
    inventory.adjust = permitido
    users.admin      = denegado

Los permisos críticos nunca dependerán únicamente del frontend.

## Storage

Supabase Storage será utilizado para archivos como:

- PDFs;
- CPE;
- documentos adjuntos;
- KML;
- GeoJSON;
- archivos de importación;
- imágenes;
- documentación operativa.

Los buckets con información privada deberán ser privados.

PostgreSQL almacenará metadata asociada a los archivos, por ejemplo:

- identificador;
- nombre;
- tipo MIME;
- tamaño;
- storage path;
- hash SHA-256;
- entidad relacionada;
- usuario;
- fecha.

## Ambientes

Se utilizarán proyectos separados.

### Staging

Proyecto Supabase exclusivo de staging.

### Producción

Proyecto Supabase exclusivo de producción.

Staging y producción no compartirán:

- base de datos;
- usuarios de servicio;
- secretos;
- buckets;
- claves privadas.

## Desarrollo local

El desarrollo local utilizará inicialmente PostgreSQL + PostGIS mediante Docker.

No será obligatorio depender de Supabase remoto para desarrollar funcionalidades básicas.

Esto permite:

- tests reproducibles;
- migrations desde cero;
- desarrollo sin conexión a infraestructura productiva;
- CI aislado.

## Reglas

- Los secretos de Supabase nunca se commitean.
- Las service role keys nunca se exponen al navegador.
- El frontend no accede a capacidades privilegiadas directamente.
- La autorización crítica se valida en Rust.
- Las migrations se mantienen en el repositorio.
- La base de staging debe poder recrearse desde migrations.
- Producción y staging permanecen físicamente separados.
- Los archivos privados no deben exponerse mediante URLs públicas permanentes sin una decisión explícita.

## Consecuencias

### Ventajas

- PostgreSQL administrado.
- PostGIS disponible.
- Auth integrado.
- Storage integrado.
- Menor carga de infraestructura.
- Separación clara entre desarrollo local y ambientes desplegados.
- Compatible con la arquitectura Rust + SQLx.

### Costos

- Existe dependencia operativa de Supabase como proveedor.
- Deben administrarse correctamente permisos y secretos.
- Auth y Storage introducen APIs externas que deben quedar aisladas detrás de infraestructura.
- Será necesario contemplar backup, restore y límites del proveedor.

## Alternativas consideradas

### PostgreSQL administrado por otro proveedor

Es técnicamente viable, pero Supabase permite consolidar PostgreSQL, Auth y Storage en una misma plataforma durante V1.

### Auth propio

No se considera necesario en V1 debido al costo y riesgo de implementar autenticación desde cero.

### Archivos dentro de PostgreSQL

No se adopta como estrategia principal para documentos y adjuntos.

## Regla

Cambiar el proveedor principal de PostgreSQL/Auth/Storage en staging o producción requiere evaluar impacto operativo y documentar la decisión mediante ADR.
