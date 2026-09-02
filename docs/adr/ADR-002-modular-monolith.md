# ADR-002: Monolito modular

## Estado

Aceptado

## Contexto

Agro Ops tendrá múltiples dominios funcionales:

- Territorio
- Stock
- Agricultura
- Maquinaria
- Ganadería
- Granos
- Comercialización
- Costos
- Integraciones con ARCA
- Integraciones con Finnegans

El sistema será utilizado inicialmente por aproximadamente 10 usuarios.

En esta etapa se priorizan simplicidad operativa, consistencia, trazabilidad y facilidad de desarrollo por encima del escalado distribuido.

Dividir el sistema tempranamente en microservicios agregaría complejidad de deployment, comunicación, observabilidad, transacciones distribuidas y mantenimiento sin una necesidad real de escala.

## Decisión

Agro Ops se implementará como un monolito modular en Rust.

Existirá una única codebase de backend con módulos internos claramente separados por responsabilidad.

La estructura conceptual será:

    services/backend/
    └── src/
        ├── bin/
        │   ├── api.rs
        │   ├── worker.rs
        │   └── jobs.rs
        ├── domain/
        ├── application/
        ├── infrastructure/
        ├── territory/
        ├── inventory/
        ├── agriculture/
        ├── machinery/
        ├── livestock/
        ├── grain/
        ├── commercial/
        ├── costs/
        ├── arca/
        ├── finnegans/
        └── common/

API, worker y jobs serán ejecutables o procesos separados, pero reutilizarán la misma lógica y los mismos módulos internos.

## Reglas de dependencia

- El dominio no depende de Axum.
- El dominio no depende de PostgreSQL o SQLx.
- El dominio no depende de ARCA.
- El dominio no depende de Finnegans.
- Los módulos de integración funcionan mediante adapters.
- La API coordina casos de uso y no concentra reglas complejas de negocio.
- Los módulos funcionales deben mantener límites claros entre sí.
- La infraestructura puede depender del dominio, pero el dominio no depende de infraestructura.

## Consecuencias

### Ventajas

- Una sola codebase para desarrollar y mantener.
- Transacciones locales simples con PostgreSQL.
- Menor complejidad de deployment.
- Reutilización directa de reglas de negocio entre API, worker y jobs.
- Tests más simples y rápidos.
- Permite mantener separación conceptual sin introducir comunicación distribuida innecesaria.

### Costos

- Será necesario controlar activamente las dependencias internas.
- Un monolito mal estructurado podría convertirse en código fuertemente acoplado.
- Los límites entre dominios deberán mantenerse mediante arquitectura, tests y revisión de código.

## Microservicios

No se utilizarán microservicios en V1.

Un módulo podrá separarse en un servicio independiente en el futuro únicamente si existen motivos concretos, como:

- necesidades de escala independientes;
- deployment independiente;
- requisitos tecnológicos diferentes;
- ownership por equipos distintos;
- aislamiento operacional necesario.

La separación requerirá un nuevo ADR.

## Regla

No crear servicios independientes ni introducir comunicación distribuida entre dominios sin una decisión arquitectónica explícita documentada mediante ADR.
