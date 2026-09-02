# ADR-006: Vercel para frontend y Railway para backend

## Estado

Aceptado

## Contexto

Agro Ops tendrá componentes con necesidades de ejecución diferentes:

- frontend web Next.js;
- API Rust persistente;
- worker Rust persistente;
- jobs programados;
- PostgreSQL/PostGIS administrado por Supabase.

El objetivo es mantener una infraestructura simple, con deployment reproducible y separación clara de responsabilidades.

## Decisión

El frontend Next.js será desplegado en Vercel.

El backend Rust será desplegado en Railway.

Railway ejecutará:

- API persistente;
- worker persistente;
- jobs programados.

La API, el worker y los jobs reutilizarán la misma codebase y la misma imagen Docker.

## Frontend

Vercel será responsable de:

- build de Next.js;
- deployment del frontend;
- previews de Pull Requests cuando corresponda;
- variables de entorno del frontend;
- deployment de producción.

El frontend no almacenará secretos privilegiados.

Las variables públicas deberán utilizar el prefijo correspondiente de Next.js únicamente cuando realmente puedan exponerse al navegador.

## Backend

Railway ejecutará la imagen Docker producida desde `services/backend`.

Ejemplos conceptuales:

    agro-api
    comando: agro-ops api

    agro-worker
    comando: agro-ops worker

    sync-arca
    comando: agro-ops job sync-arca

    reconcile
    comando: agro-ops job reconcile

Los procesos comparten código, pero tienen ciclos de vida independientes.

## API

La API será un proceso persistente.

Será responsable de:

- recibir requests HTTP;
- autenticación y autorización;
- coordinación de casos de uso;
- validaciones;
- transacciones;
- respuestas al frontend.

## Worker

El worker será un proceso persistente.

Será responsable de:

- transactional outbox;
- retries;
- integración con sistemas externos;
- procesamiento asíncrono;
- trabajos que no deben bloquear requests de usuario.

## Jobs programados

Los jobs serán procesos ejecutados por cron o scheduler.

Ejemplos futuros:

- sincronización ARCA;
- conciliación con Finnegans;
- detección de documentos pendientes;
- generación de alertas;
- tareas de mantenimiento.

## Ambientes

Se mantendrán ambientes separados.

### Staging

- Vercel staging/preview;
- Railway staging;
- Supabase staging.

### Producción

- Vercel production;
- Railway production;
- Supabase production.

Las variables y secretos no se compartirán entre staging y producción.

## Reglas

- El frontend no se conecta directamente a PostgreSQL con permisos privilegiados.
- Las reglas críticas de negocio viven en el backend Rust.
- API y worker no se fusionan en un único proceso de producción.
- Los jobs programados no se implementan como endpoints HTTP públicos.
- La misma imagen Docker del backend debe poder ejecutar API, worker y jobs.
- Los deployments deben generarse desde código versionado en GitHub.
- Los secretos se configuran mediante las capacidades del proveedor y nunca se commitean.

## Consecuencias

### Ventajas

- Vercel tiene integración directa con Next.js.
- Railway simplifica la ejecución de procesos Rust persistentes y jobs.
- Separación clara entre frontend y backend.
- La misma imagen backend puede reutilizarse en distintos procesos.
- Staging y producción pueden mantenerse separados.
- Menor carga operativa que administrar servidores propios.

### Costos

- Dependencia operativa de dos proveedores de deployment.
- Será necesario administrar variables y secretos en más de una plataforma.
- Se deberán observar límites, costos y comportamiento de cada proveedor.
- La conectividad Railway-Supabase deberá ser monitorizada.

## Alternativas consideradas

### Ejecutar todo en un único servidor

Es posible, pero aumenta la carga de administración y reduce la independencia entre frontend, API, worker y jobs.

### Desplegar Next.js junto al backend Rust

No se considera necesario en V1 y perdería varias ventajas del ecosistema de Vercel para Next.js.

### Serverless para todo el backend

No se selecciona porque Agro Ops necesita workers persistentes y procesos programados, además de la API.

## Regla

Cambiar la estrategia principal de deployment requiere un nuevo ADR o una actualización explícita de esta decisión.
