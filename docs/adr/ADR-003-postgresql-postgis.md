# ADR-003: PostgreSQL + PostGIS como base operativa

## Estado

Aceptado

## Contexto

Agro Ops necesita almacenar información operativa fuertemente relacionada:

- establecimientos;
- campañas;
- lotes;
- unidades operativas;
- productos;
- depósitos;
- movimientos de stock;
- labores;
- maquinaria;
- ganadería;
- contratos;
- CPE;
- certificados;
- liquidaciones;
- auditoría;
- jobs;
- outbox.

El sistema también necesita manejar información geográfica real:

- polígonos de lotes;
- subdivisiones por campaña;
- superficies;
- contención;
- intersecciones;
- solapamientos;
- consultas espaciales.

Además, varias reglas críticas dependen de transacciones consistentes y constraints de base de datos.

## Decisión

PostgreSQL será la base de datos relacional principal de Agro Ops.

PostGIS será habilitado como extensión geoespacial de PostgreSQL.

PostgreSQL será utilizado tanto para datos de negocio como para capacidades transversales como:

- auditoría;
- transactional outbox;
- jobs;
- idempotencia;
- referencias externas.

## Modelo geoespacial

Las geometrías territoriales se almacenarán mediante tipos PostGIS.

Ejemplos:

- establecimiento: Polygon o MultiPolygon cuando corresponda;
- lote base: Polygon o MultiPolygon;
- unidades operativas: Polygon o MultiPolygon;
- puntos de interés: Point cuando corresponda.

Las geometrías deberán utilizar un SRID explícito y consistente.

## Reglas

- No almacenar geometrías críticas solamente como texto, JSON o arrays de coordenadas.
- Las validaciones espaciales relevantes deben ejecutarse también en backend/base de datos.
- El frontend no es autoridad para validar áreas o solapamientos.
- Las superficies derivadas de geometrías deberán ser reproducibles.
- Las migrations de schema deberán estar versionadas.
- Las reglas críticas deberán respaldarse con constraints cuando sea razonable.
- Las operaciones que modifican múltiples registros relacionados deberán utilizar transacciones.
- PostgreSQL será la fuente de verdad persistente de Agro Ops.

## Acceso desde Rust

El backend utilizará SQLx para interactuar con PostgreSQL.

El dominio no dependerá directamente de SQLx ni de PostgreSQL.

La infraestructura será responsable de:

- queries;
- repositories;
- transactions;
- migrations;
- persistencia.

## Entornos

### Local

PostgreSQL + PostGIS se ejecutará mediante Docker.

### Staging

Se utilizará un proyecto Supabase independiente.

### Producción

Se utilizará un proyecto Supabase de producción independiente.

Staging y producción no compartirán bases, usuarios ni secretos.

## Consecuencias

### Ventajas

- Transacciones ACID.
- Constraints e integridad referencial.
- Soporte robusto para concurrencia.
- Consultas SQL expresivas.
- Soporte geoespacial maduro mediante PostGIS.
- Un mismo motor puede cubrir negocio, auditoría, jobs y outbox.
- Reduce la necesidad de introducir infraestructura adicional en V1.

### Costos

- Las migrations deben mantenerse cuidadosamente.
- Las queries críticas deben probarse contra PostgreSQL real.
- Las geometrías requieren decisiones correctas de SRID y proyección.
- Algunas operaciones espaciales pueden requerir índices específicos.

## Alternativas consideradas

### Base NoSQL

No se selecciona como almacenamiento principal debido a la importancia de relaciones, constraints, transacciones y consultas estructuradas.

### Base geoespacial separada

No se considera necesaria en V1 porque PostGIS cubre las necesidades previstas dentro del mismo PostgreSQL.

## Regla

Cambiar PostgreSQL como base principal o separar la información geográfica en otro motor requiere un nuevo ADR.
