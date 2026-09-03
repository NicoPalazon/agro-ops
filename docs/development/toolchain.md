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
| actions/checkout | v7 |

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
