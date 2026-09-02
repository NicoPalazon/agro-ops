# ADR-010: Finnegans detrás de un adapter y sin dependencia de BProc

## Estado

Aceptado

## Contexto

Agro Ops coexistirá con Finnegans.

Cada sistema tendrá responsabilidades diferentes.

Agro Ops será la fuente de verdad para:

- operación agropecuaria;
- territorio;
- campañas;
- labores;
- stock operativo;
- maquinaria;
- ganadería;
- granos;
- contratos operativos;
- costos y resultados productivos.

Finnegans continuará siendo la fuente de verdad para:

- contabilidad;
- fiscalidad;
- impuestos;
- tesorería;
- bancos;
- proveedores y clientes;
- facturas;
- pagos;
- liquidaciones fiscales;
- registración patrimonial.

Ambos sistemas deberán intercambiar información sin crear una dependencia estructural entre el dominio de Agro Ops y los detalles internos de Finnegans.

El proyecto tiene además una restricción explícita: la solución no debe depender de BProc.

## Decisión

Toda interacción entre Agro Ops y Finnegans se realizará detrás de un adapter específico.

Conceptualmente:

    Agro Ops Domain
          |
          v
    Application Layer
          |
          v
    Finnegans Port
          |
          v
    Finnegans Adapter
          |
          v
    API / import-export /
    mecanismo estándar soportado
          |
          v
       Finnegans

El dominio de Agro Ops no conocerá:

- IDs internos de Finnegans;
- estructuras específicas de documentos de Finnegans;
- nombres de tablas internas;
- workflows internos;
- BProc;
- detalles técnicos de autenticación;
- formatos específicos de importación o exportación.

## Responsabilidad del adapter

El adapter Finnegans será responsable de:

- autenticación;
- comunicación con Finnegans;
- serialización y parsing;
- traducción entre modelos;
- manejo de errores;
- retries cuando corresponda;
- idempotencia;
- referencias externas;
- adaptación a los mecanismos estándar disponibles.

## Ports

La capa de aplicación dependerá de interfaces o ports definidos desde Agro Ops.

Ejemplo conceptual:

    trait ExternalCostRepository

o:

    trait FinnegansGateway

La implementación concreta vivirá en infraestructura.

Esto permite que el dominio exprese necesidades propias sin depender del contrato externo.

## ExternalReference

Las entidades que necesiten trazabilidad con Finnegans podrán mantener referencias externas.

Ejemplo conceptual:

    external_system = "finnegans"
    external_id = "..."
    external_type = "..."
    sync_version = "..."
    last_sync_status = "..."
    last_synced_at = "..."

Los IDs de Finnegans no serán utilizados como identificadores primarios internos de Agro Ops.

Las entidades de Agro Ops utilizarán UUID internos estables.

## Flujo Agro Ops hacia Finnegans

Agro Ops enviará solamente las consecuencias que Finnegans necesite.

No se replicará indiscriminadamente toda la operación.

Ejemplos potenciales:

- consecuencias patrimoniales de stock;
- producción cuando el POC determine que debe reflejarse;
- referencias necesarias para conciliación;
- otros eventos reales definidos explícitamente.

No se enviarán como transacciones contables:

- labores propias ficticias;
- tarifas internas de maquinaria;
- ingresos internos por servicios propios;
- valorizaciones utilizadas únicamente para Margen Bruto.

## Flujo Finnegans hacia Agro Ops

Agro Ops podrá recibir desde Finnegans información como:

- compras;
- facturas;
- costos reales;
- gastos;
- reparaciones;
- fletes;
- compras o ventas de hacienda;
- liquidaciones fiscales de granos;
- documentos contables que tengan consecuencia operativa.

El adapter deberá convertir esos documentos en información operativa relevante sin reproducir innecesariamente toda la estructura contable.

## Liquidaciones de granos

La liquidación fiscal oficial continuará perteneciendo a Finnegans.

Agro Ops podrá importar información relevante como:

- kilos;
- precio;
- descuentos;
- gastos;
- contraparte;
- contrato relacionado;
- referencias a CPE;
- referencias a certificados.

Agro Ops no volverá a emitir la liquidación fiscal.

## Stock

Agro Ops será dueño del ledger operativo.

Finnegans conservará la representación patrimonial que corresponda.

Las integraciones deberán permitir conciliación entre ambas representaciones sin imponer que tengan exactamente el mismo modelo interno.

## BProc

BProc no será utilizado como dependencia de la solución Agro Ops.

No se diseñarán integraciones cuya continuidad dependa de:

- crear BProc;
- modificar BProc;
- ejecutar lógica personalizada mediante BProc.

Las integraciones deberán utilizar mecanismos soportados y mantenibles, como:

- APIs;
- servicios;
- import/export;
- interfaces estándar disponibles;
- mecanismos configurables soportados por Finnegans.

El mecanismo técnico definitivo deberá validarse mediante POC.

## Idempotencia

Toda integración deberá asumir que un mensaje o documento puede recibirse más de una vez.

Ejemplo:

    importar factura X
    importar factura X
    importar factura X

debe producir un único efecto operativo.

Cuando corresponda se utilizarán:

- external_id;
- idempotency keys;
- versiones;
- constraints;
- referencias externas.

## Modificaciones y anulaciones

El adapter deberá contemplar no solamente altas, sino también:

- modificaciones;
- anulaciones;
- correcciones;
- reintentos.

No se considerará completa una integración que funcione únicamente para creación inicial.

## Fallos

Una caída de Finnegans no deberá provocar pérdida de una operación ya confirmada en Agro Ops.

Las operaciones asíncronas utilizarán:

- transactional outbox;
- worker;
- jobs;
- retries;
- backoff;
- estados observables.

Los errores de integración deberán permanecer visibles.

## Mapeos

Cuando Agro Ops y Finnegans utilicen códigos maestros diferentes, los mapeos deberán ser explícitos.

Ejemplos:

- productos;
- establecimientos;
- depósitos;
- terceros;
- centros de costo;
- campañas;
- documentos.

No se deberán inferir mapeos críticos únicamente comparando descripciones libres.

## Pruebas obligatorias

Cada integración crítica deberá cubrir:

- alta;
- modificación;
- anulación;
- retry;
- idempotencia;
- timeout;
- error de autenticación;
- documento duplicado;
- mapeo faltante;
- referencia externa inválida;
- caída temporal de Finnegans.

Además deberán existir POC específicos para:

- stock de insumos;
- cosecha y stock de granos;
- liquidaciones;
- fletes;
- maquinaria;
- ganadería.

## Consecuencias

### Ventajas

- El dominio permanece independiente de Finnegans.
- Agro Ops puede evolucionar aunque cambie el mecanismo de integración.
- Los tests pueden utilizar adapters falsos o mocks.
- Reduce propagación de conceptos específicos del ERP.
- Facilita reemplazar el mecanismo técnico de integración.
- Evita dependencia de BProc.
- Mejora trazabilidad y conciliación.

### Costos

- Es necesario mantener una capa adicional de adaptación.
- Los mappings deben administrarse explícitamente.
- Las diferencias conceptuales entre ambos sistemas deben resolverse caso por caso.
- Será necesario realizar POC antes de cerrar ciertos circuitos.

## Regla

Ningún módulo de dominio de Agro Ops podrá depender directamente de Finnegans.

Toda integración deberá pasar por un adapter y utilizar mecanismos estándar soportados.

BProc no forma parte de la arquitectura de Agro Ops.

Modificar esta decisión requiere un nuevo ADR.
