# ADR-001: Rust + Axum para el backend

## Estado

Aceptado

## Contexto

Agro Ops necesita un backend confiable para manejar reglas de negocio, persistencia, integraciones, workers y jobs programados.

El sistema debe priorizar consistencia, trazabilidad, concurrencia segura y facilidad para construir procesos persistentes y asíncronos.

## Decisión

El backend de Agro Ops se implementará en Rust.

El framework HTTP será Axum.

La ejecución asíncrona utilizará Tokio.

El acceso a PostgreSQL utilizará SQLx.

## Consecuencias

- Las reglas de negocio críticas se implementan en Rust.
- API, worker y jobs reutilizan la misma codebase.
- El dominio debe mantenerse independiente de Axum.
- Axum se utiliza únicamente en la capa de entrada HTTP.
- SQLx se utiliza desde la infraestructura y persistencia, no desde las entidades puras de dominio.
- Tokio provee el runtime asíncrono para API, workers e integraciones.

## Alternativas consideradas

### Node.js / TypeScript

Es adecuado para aplicaciones web, pero no fue seleccionado como backend principal.

TypeScript continuará utilizándose en el frontend.

### Python

Puede incorporarse en el futuro para ML, análisis o procesamiento especializado, pero no será el backend principal de V1.

## Regla

Cambiar el lenguaje o framework principal del backend requiere un nuevo ADR que reemplace explícitamente esta decisión.
