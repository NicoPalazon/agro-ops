# Agro Ops — Toolchain

Baseline técnico aprobado para el desarrollo de Agro Ops.

## Frontend

| Componente | Versión | Estado |
| --- | --- | --- |
| Node.js | 24.20.0 LTS | Fijada |
| npm | 12.0.2 | Fijada |
| pnpm | 11.25.0 | Fijada |
| Next.js | 16.3.4 | Fijada |
| React | 19.2.8 | Fijada |
| React DOM | 19.2.8 | Fijada |
| TypeScript | 5.9.3 | Fijada |
| ESLint | 9.39.5 | Excepción temporal de compatibilidad |
| eslint-config-next | 16.3.4 | Fijada |
| @types/node | 24.13.3 | Fijada y alineada con Node 24 |
| @types/react | 19.2.18 | Fijada |
| @types/react-dom | 19.2.5 | Fijada |

## Contenedores y CI

| Componente | Versión |
| --- | --- |
| Docker Engine | 29.7.2 |
| Docker Compose | 5.5.0 |
| SQLx CLI | 0.9.0 (PostgreSQL) |
| actions/checkout | v7 |

## Migrations del backend

SQLx CLI está disponible en la imagen `dev` del backend. Los comandos se ejecutan
desde `/app`, donde Compose monta `services/backend`.

Crear una migration:

    docker compose run --rm api sqlx migrate add <nombre> --source migrations

Ejecutar migrations y consultar su estado:

    docker compose run --rm api sqlx migrate run --source migrations
    docker compose run --rm api sqlx migrate info --source migrations

Para probar todas las migrations desde una base vacía sin modificar `agro_ops`:

    docker compose exec postgres dropdb --if-exists -U agro_ops agro_ops_migration_test
    docker compose exec postgres createdb -U agro_ops agro_ops_migration_test
    docker compose run --rm -e DATABASE_URL=postgres://agro_ops:agro_ops@postgres:5432/agro_ops_migration_test api sqlx migrate run --source migrations
    docker compose exec postgres psql -U agro_ops -d agro_ops_migration_test -c 'TABLE _sqlx_migrations;'
    docker compose exec postgres psql -U agro_ops -d agro_ops_migration_test -c 'SELECT PostGIS_Version();'
    docker compose exec postgres dropdb -U agro_ops agro_ops_migration_test

## Política de versiones

Agro Ops utiliza la versión estable más reciente compatible con el stack aprobado.

Las versiones relevantes deben quedar fijadas en los archivos ejecutables correspondientes, incluyendo package.json, pnpm-lock.yaml, Dockerfile, Compose y workflows de GitHub Actions.

Este documento funciona como índice humano del toolchain y no reemplaza esos archivos como source of truth ejecutable.

## Política de package manager

pnpm es el package manager oficial del frontend.

npm forma parte del entorno Node.js y se fija en la imagen Docker, pero no se utiliza para administrar las dependencias del proyecto.

## Build scripts de dependencias

pnpm exige aprobación explícita para determinados scripts de instalación.

Actualmente se autoriza:

- unrs-resolver

La autorización queda registrada en apps/web/pnpm-workspace.yaml.

## Excepciones conocidas

### ESLint

ESLint 10.9.1 fue evaluado.

Actualmente presenta incompatibilidades de peer dependencies con plugins utilizados por eslint-config-next 16.3.4, incluyendo eslint-plugin-import, eslint-plugin-jsx-a11y y eslint-plugin-react.

Por compatibilidad, Agro Ops mantiene temporalmente ESLint 9.39.5.

Esta versión debe actualizarse cuando el stack oficial utilizado por Next.js soporte ESLint 10 sin conflictos.

## Validaciones del baseline

El baseline fue verificado mediante:

- pnpm peers check
- TypeScript typecheck con tsc --noEmit
- ESLint

Las tres validaciones finalizaron correctamente.
