# ADR-004: Docker como entorno reproducible

## Estado

Aceptado

## Contexto

Agro Ops utiliza múltiples tecnologías:

- Next.js / Node.js para frontend;
- Rust para backend;
- PostgreSQL + PostGIS para datos;
- procesos separados para API y worker;
- herramientas de build, migrations y tests.

Instalar y mantener manualmente todas estas dependencias en cada máquina de desarrollo aumenta el riesgo de diferencias de versiones y problemas de reproducibilidad.

El objetivo es que una nueva máquina pueda clonar el repositorio y levantar un entorno coherente con la menor cantidad posible de dependencias globales.

## Decisión

Docker será el mecanismo estándar para construir y ejecutar los componentes de Agro Ops.

Docker Compose será utilizado para levantar el entorno local completo.

La máquina anfitriona necesitará principalmente:

- Git;
- Docker;
- Docker Compose;
- editor o herramientas de desarrollo.

## Entorno local previsto

Docker Compose deberá poder ejecutar al menos:

    web-dev
    api-dev
    worker-dev
    postgres-dev

Donde:

- web-dev ejecuta Next.js;
- api-dev ejecuta la API Rust/Axum;
- worker-dev ejecuta el worker Rust;
- postgres-dev ejecuta PostgreSQL + PostGIS.

## Backend

El backend utilizará un Dockerfile multi-stage.

La misma codebase e imagen deberán poder ejecutar distintos procesos:

    agro-ops api
    agro-ops worker
    agro-ops job <nombre-del-job>

API, worker y jobs no utilizarán imágenes independientes salvo que exista una razón arquitectónica futura explícita.

## Desarrollo

El entorno de desarrollo deberá:

- permitir hot reload cuando sea razonable;
- montar el código fuente necesario;
- exponer únicamente los puertos requeridos;
- utilizar variables de entorno;
- permitir aplicar migrations;
- permitir ejecutar tests dentro de un entorno reproducible.

## Producción

El Dockerfile deberá generar una imagen runtime pequeña y sin herramientas de desarrollo innecesarias.

La misma imagen será utilizada para desplegar:

- API;
- worker;
- jobs programados.

El comando de arranque determinará el proceso ejecutado.

## Reglas

- Los secretos no se incorporan a imágenes Docker.
- Los archivos `.env` reales no se commitean.
- Las imágenes deben poder construirse desde cero en CI.
- Las dependencias deben estar versionadas.
- No depender de software instalado manualmente en la máquina cuando pueda definirse dentro del entorno Docker.
- El build de producción debe validarse en GitHub Actions.
- El entorno local debe aproximarse razonablemente al entorno desplegado.

## Consecuencias

### Ventajas

- Entorno reproducible entre desarrolladores.
- Menos problemas por diferencias de versiones.
- Onboarding más simple.
- Build local y CI más consistentes.
- Misma imagen de backend para API, worker y jobs.
- Menor diferencia entre desarrollo y producción.

### Costos

- Docker agrega una capa de complejidad operativa.
- El hot reload puede requerir configuración específica.
- En Windows, el rendimiento de volúmenes puede diferir del de Linux.
- Será necesario mantener Dockerfiles y Docker Compose junto con el código.

## Alternativas consideradas

### Instalación manual de dependencias

No se adopta como flujo principal porque aumenta la variabilidad entre entornos.

### Máquinas virtuales completas

No se consideran necesarias para el alcance inicial.

## Regla

Cambiar Docker como entorno reproducible principal requiere un nuevo ADR o una actualización explícita de esta decisión.
