# Agro Ops

Sistema operativo agropecuario de Pilar del Tala S.A.

## Objetivo

Agro Ops será la fuente de verdad operativa y productiva de la empresa.

La separación de responsabilidades es:

- **Agro Ops:** operación agropecuaria y gestión productiva.
- **Finnegans:** contabilidad, fiscalidad, tesorería e impuestos.
- **ARCA:** fuente legal de Carta de Porte Electrónica.

## Arquitectura

### Frontend

- Next.js
- React
- TypeScript
- MapLibre
- PWA online-first
- Deploy: Vercel

### Backend

- Rust
- Axum
- Tokio
- SQLx
- Monolito modular
- API, worker y jobs como procesos separados desde una misma codebase
- Deploy: Railway

### Datos

- PostgreSQL
- PostGIS
- Supabase en staging y producción
- Supabase Auth
- Supabase Storage

### Ingeniería

- GitHub como fuente de verdad del código
- Docker como entorno reproducible
- GitHub Actions para CI
- Tests automáticos en cada push
- Main protegida
- Cambios mediante branches y Pull Requests

## Estructura prevista

    agro-ops/
    ├── apps/
    │   └── web/
    ├── services/
    │   └── backend/
    ├── docs/
    │   └── adr/
    ├── .github/
    │   └── workflows/
    ├── .env.example
    ├── .gitignore
    └── README.md

## Principios

- Cada dato tiene un sistema dueño explícito.
- Un evento real se carga manualmente una sola vez.
- Las operaciones confirmadas no se borran destructivamente: se anulan o revierten.
- Las integraciones deben ser idempotentes y auditables.
- Dinero, kg, litros, hectáreas, horas y km no usan floating point como representación crítica.
- El dominio no depende de Finnegans, ARCA, Axum ni infraestructura externa.
- Finnegans se integra únicamente mediante un adapter.
- La solución no depende de BProc.
- Una feature no está terminada hasta que sus tests y CI estén en verde.

## Estado

Proyecto en bootstrap inicial.

Primer objetivo técnico: construir el walking skeleton con:

1. Next.js
2. Rust/Axum
3. PostgreSQL/PostGIS
4. Worker Rust
5. Docker Compose
6. GitHub Actions
7. Staging
