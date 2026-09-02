# ADR-012: GitHub + CI obligatorio como gate de calidad

## Estado

Aceptado

## Contexto

Agro Ops será desarrollado incrementalmente durante múltiples etapas.

El sistema tendrá reglas críticas relacionadas con:

- stock;
- granos;
- contratos;
- campañas;
- integraciones;
- costos;
- auditoría;
- idempotencia;
- concurrencia.

A medida que el sistema crezca, un cambio en un módulo podría romper comportamiento existente en otro.

Además, parte del desarrollo podrá realizarse con asistencia de herramientas como Codex.

Por lo tanto, el proyecto necesita una barrera automática y reproducible que valide cada cambio antes de incorporarlo a la rama principal.

## Decisión

GitHub será la fuente de verdad del código de Agro Ops.

La rama `main` estará protegida.

Todo cambio deberá seguir el flujo:

    branch
      ↓
    desarrollo
      ↓
    tests locales
      ↓
    commit
      ↓
    push
      ↓
    GitHub Actions
      ↓
    Pull Request
      ↓
    review
      ↓
    merge a main

No se trabajará directamente sobre `main` como flujo normal.

## Branches

Los cambios utilizarán branches con nombres descriptivos.

Ejemplos:

    feat/territory-base-plots
    feat/inventory-ledger
    feat/arca-import
    fix/grain-reversal
    chore/project-bootstrap

## Pull Requests

Todo cambio significativo deberá entrar mediante Pull Request.

El PR deberá permitir entender:

- qué cambia;
- por qué cambia;
- qué tests lo cubren;
- qué riesgos existen;
- si modifica arquitectura;
- si requiere migration;
- si afecta integraciones.

## Main protegida

La rama `main` deberá configurarse para:

- impedir push directo cuando sea posible;
- requerir Pull Request;
- requerir checks exitosos;
- impedir merge cuando un required check falla;
- mantener historial de cambios revisable.

## GitHub Actions

GitHub Actions será el sistema principal de CI.

Cada push o Pull Request deberá ejecutar progresivamente los checks relevantes.

## Backend Rust

Los checks mínimos incluirán:

    cargo fmt --check

    cargo clippy --all-targets --all-features -- -D warnings

    cargo check

    cargo test

A medida que el proyecto crezca se agregarán:

- unit tests;
- domain tests;
- property-based tests;
- integration tests;
- contract tests;
- mutation tests selectivos.

## Base de datos

CI deberá levantar PostgreSQL + PostGIS real para pruebas que dependan de comportamiento de base.

Los checks incluirán:

- crear base desde cero;
- aplicar migrations;
- validar constraints;
- ejecutar integration tests;
- validar queries SQLx;
- probar transacciones cuando corresponda.

No se deberán mockear comportamientos cuyo resultado dependa de PostgreSQL real.

## Frontend

Los checks mínimos incluirán:

- lint;
- TypeScript typecheck;
- unit tests;
- component tests;
- build de Next.js.

Más adelante se incorporarán E2E de los flujos críticos.

## Docker

CI deberá construir la imagen Docker de producción.

El build debe demostrar que el proyecto puede compilarse desde cero sin depender de archivos locales no versionados.

Se agregarán smoke tests de arranque cuando el backend esté disponible.

## E2E

Los flujos operativos críticos deberán incorporar pruebas end-to-end progresivamente.

Ejemplos futuros:

    login
      ↓
    mapa
      ↓
    seleccionar UOP
      ↓
    registrar labor
      ↓
    consumir stock
      ↓
    validar saldo

y:

    contrato
      ↓
    CPE
      ↓
    certificado
      ↓
    liquidación
      ↓
    conciliación

## Required checks

Los checks críticos de GitHub Actions deberán configurarse como required checks para `main`.

Si un check obligatorio falla:

    CI = FAILED

entonces:

    MERGE = BLOQUEADO

## Definition of Done

Una pantalla funcionando no significa que una feature esté terminada.

Una feature se considera completa cuando cumple, según corresponda:

- arquitectura respetada;
- invariantes explícitas;
- migrations versionadas;
- API contract documentado;
- permisos;
- tests unitarios;
- tests de integración;
- audit;
- idempotencia;
- outbox;
- frontend tests;
- E2E;
- Docker build;
- CI verde;
- staging validado;
- documentación actualizada.

## Codex y agentes de desarrollo

Las herramientas automáticas deberán respetar la arquitectura existente.

No deberán:

- modificar decisiones arquitectónicas silenciosamente;
- introducir nuevas tecnologías sin ADR;
- eliminar tests para hacer pasar CI;
- desactivar validaciones;
- ignorar errores de lint;
- modificar contratos existentes sin actualizar sus tests.

CI funciona como barrera objetiva frente a regresiones producidas tanto por humanos como por agentes.

## ADR

Un cambio arquitectónico significativo deberá documentarse mediante ADR antes o junto con su implementación.

Ejemplos:

- cambiar base de datos;
- introducir Redis;
- introducir Kafka;
- separar microservicio;
- cambiar framework;
- cambiar estrategia de deployment;
- introducir offline-first;
- cambiar modelo de ledger.

## Consecuencias

### Ventajas

- Menor riesgo de regresiones.
- Historial completo de cambios.
- Arquitectura controlada.
- Integración continua reproducible.
- Mayor seguridad al utilizar herramientas automáticas de desarrollo.
- Facilita colaboración futura.
- Permite refactorizar con mayor confianza.

### Costos

- Cada cambio requiere disciplina.
- Los Pull Requests agregan un paso adicional.
- La suite de CI consume tiempo de ejecución.
- Tests frágiles o lentos deberán mantenerse cuidadosamente.
- El pipeline crecerá en complejidad con el sistema.

## Regla

No se considera terminada una feature si sus checks obligatorios no están en verde.

No se mergea código a `main` con required checks fallando.

Cambiar esta política requiere una decisión explícita documentada mediante ADR.
