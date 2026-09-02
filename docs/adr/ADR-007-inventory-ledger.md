# ADR-007: Ledger de movimientos para stock de insumos y granos

## Estado

Aceptado

## Contexto

Agro Ops necesita gestionar stock operativo de:

- insumos;
- combustibles cuando corresponda;
- granos;
- movimientos entre depósitos;
- consumos;
- ingresos;
- ajustes;
- reversión de operaciones.

El sistema debe poder explicar en todo momento por qué existe un saldo determinado.

Guardar únicamente un campo como:

    stock_actual = 5420

no permite reconstruir de manera confiable:

- qué operaciones generaron ese saldo;
- quién las realizó;
- qué documento fue el origen;
- qué operación fue anulada;
- qué cantidad se transfirió entre depósitos;
- cómo se llegó históricamente al saldo actual.

## Decisión

El stock operativo de Agro Ops se modelará mediante un ledger de movimientos.

El saldo será derivado de movimientos confirmados y no será la única fuente de verdad.

Ejemplo:

    Ingreso compra          +5000 L
    Consumo labor           -280 L
    Transferencia salida    -500 L
    Transferencia entrada   +500 L
    Ajuste                  +50 L

Cada movimiento deberá conservar su origen y trazabilidad.

## Tipos de movimientos

El modelo deberá soportar al menos:

- ingreso;
- salida;
- consumo;
- transferencia;
- ajuste;
- reversión.

Podrán agregarse tipos específicos de dominio cuando exista una necesidad funcional explícita.

## Saldo

El saldo por producto y depósito será calculado como la suma de movimientos confirmados.

Conceptualmente:

    saldo = suma(movimientos confirmados)

No se permitirá modificar el saldo directamente sin generar un movimiento que explique el cambio.

## Transferencias

Una transferencia entre depósitos deberá producir efectos equivalentes a:

    depósito origen   -cantidad
    depósito destino  +cantidad

El total global del producto no deberá modificarse por una transferencia interna.

## Reversión

Las operaciones confirmadas no se eliminarán destructivamente.

Una anulación deberá generar la reversión correspondiente.

Ejemplo:

    consumo original   -280 L
    reversión           +280 L

La historia original permanece disponible.

## Trazabilidad

Cada movimiento deberá poder vincularse con información como:

- producto;
- depósito;
- cantidad;
- unidad;
- tipo;
- fecha;
- documento o evento origen;
- usuario;
- estado;
- operación relacionada;
- movimiento revertido cuando corresponda;
- idempotency key cuando corresponda.

## Precisión numérica

Las cantidades críticas no utilizarán floating point como representación persistente.

Se utilizarán tipos Decimal adecuados para:

- kg;
- litros;
- hectáreas;
- unidades;
- otras cantidades operativas.

Las reglas de redondeo deberán ser explícitas.

## Concurrencia

Las operaciones que puedan competir por el mismo stock deberán protegerse mediante transacciones y mecanismos adecuados de PostgreSQL.

Dos requests concurrentes no deberán poder producir un estado inválido cuando las reglas de negocio prohíban stock negativo.

## Idempotencia

Una misma operación repetida por:

- retry de red;
- doble click;
- reinicio del worker;
- reintento de integración;

no deberá generar movimientos duplicados cuando exista una misma idempotency key.

## Granos

El mismo principio de ledger será reutilizado para stock físico de granos.

Ejemplos de origen:

- cosecha;
- traslado;
- ajuste;
- certificado cuando corresponda al circuito;
- liquidación cuando corresponda;
- reversión.

Las reglas comerciales de contratos, CPE, certificados y liquidaciones se modelarán aparte y no reemplazarán el ledger físico.

## Relación con Finnegans

Agro Ops será la fuente de verdad del stock operativo.

Finnegans conservará la representación patrimonial y contable que corresponda.

Las diferencias entre ambos sistemas deberán ser conciliables.

No se enviarán a Finnegans movimientos ficticios derivados únicamente de valorizaciones internas.

## Reglas

- No modificar saldos directamente.
- No borrar movimientos confirmados.
- Toda corrección operativa debe ser explicable.
- Las transferencias deben conservar cantidad global.
- Las reversiones deben conservar historia.
- Las cantidades críticas utilizan Decimal.
- Las operaciones críticas deben ser transaccionales.
- Los retries no deben duplicar efectos.
- Todo movimiento debe conservar referencia a su origen.

## Pruebas obligatorias

La implementación deberá demostrar mediante tests:

- saldo igual a suma de movimientos;
- transferencia conserva cantidad global;
- consumo reduce saldo exactamente;
- reversión restaura saldo;
- request duplicada no duplica movimiento;
- concurrencia no viola reglas de stock;
- límites exactos de cantidad;
- comportamiento correcto de Decimal;
- trazabilidad hacia documento origen.

## Consecuencias

### Ventajas

- Auditoría completa.
- Saldo explicable.
- Reversión segura.
- Conciliación más simple.
- Mejor soporte para integraciones.
- Permite reconstruir estado histórico.
- Reduce modificaciones destructivas.

### Costos

- El modelo es más complejo que almacenar un único saldo.
- Será necesario optimizar consultas de saldo cuando aumente el volumen.
- Las reglas de concurrencia deberán diseñarse cuidadosamente.
- Las correcciones requieren movimientos explícitos.

## Alternativas consideradas

### Campo de saldo mutable

No se adopta como fuente principal porque pierde trazabilidad y dificulta auditoría, reversión y conciliación.

### Event sourcing completo

No se adopta para toda la aplicación en V1.

El ledger utiliza principios similares para cantidades, pero Agro Ops no será un sistema event-sourced completo.

## Regla

Cambiar el modelo de stock para utilizar saldos mutables como única fuente de verdad requiere un nuevo ADR.
