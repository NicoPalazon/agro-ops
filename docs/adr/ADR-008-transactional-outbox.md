# ADR-008: Transactional Outbox + Jobs en PostgreSQL

## Estado

Aceptado

## Contexto

Agro Ops deberá integrarse con sistemas externos como:

- ARCA;
- Finnegans;
- servicios de almacenamiento;
- otros sistemas futuros.

Las operaciones externas pueden fallar por:

- timeout;
- problemas de red;
- credenciales vencidas;
- indisponibilidad temporal;
- errores de terceros;
- reinicios del worker.

No es seguro realizar una operación de negocio y luego depender de una llamada externa dentro de la misma request como única forma de completar el proceso.

Ejemplo problemático:

    Usuario confirma una cosecha
        ↓
    Agro Ops guarda cosecha
        ↓
    Agro Ops llama Finnegans
        ↓
    Finnegans está caído
        ↓
    estado incierto

También existe el problema inverso:

    Agro Ops llama Finnegans
        ↓
    Finnegans confirma
        ↓
    Agro Ops falla antes de guardar localmente
        ↓
    sistemas inconsistentes

## Decisión

Agro Ops utilizará el patrón Transactional Outbox para eventos que deban ser procesados de manera asíncrona.

El cambio de negocio y el registro del evento de outbox deberán confirmarse dentro de la misma transacción PostgreSQL.

Conceptualmente:

    BEGIN

    guardar operación de negocio

    guardar audit event

    guardar outbox event

    COMMIT

Una vez confirmado el commit, un worker procesa los eventos pendientes.

## Transactional Outbox

La tabla de outbox almacenará eventos que todavía deben producir un efecto externo o asíncrono.

Cada evento deberá contener como mínimo información equivalente a:

- id;
- event_type;
- aggregate_type;
- aggregate_id;
- payload;
- status;
- idempotency_key;
- created_at;
- next_attempt_at;
- attempts;
- last_error.

Los detalles exactos del schema se definirán durante la implementación.

## Estados

El modelo deberá soportar estados equivalentes a:

    PENDING
    PROCESSING
    SUCCEEDED
    FAILED
    DEAD

Los nombres finales podrán ajustarse, pero las transiciones deberán ser explícitas.

## Worker

El worker será un proceso persistente separado de la API.

Su responsabilidad será:

- buscar eventos pendientes;
- bloquearlos de forma segura;
- ejecutar adapters;
- registrar resultado;
- realizar retries;
- aplicar backoff;
- mover eventos irrecuperables a estado DEAD;
- preservar información de error.

## Jobs

Agro Ops utilizará también un sistema de jobs persistidos en PostgreSQL.

Los jobs serán utilizados para tareas como:

- sincronización periódica con ARCA;
- conciliación con Finnegans;
- detección de CPE nuevas;
- generación de alertas;
- reintentos;
- mantenimiento programado.

Los jobs deberán soportar información equivalente a:

- tipo;
- estado;
- scheduled_at;
- next_attempt_at;
- attempts;
- last_error;
- payload.

## Idempotencia

Los consumers y adapters deberán asumir que un evento puede procesarse más de una vez.

Por lo tanto:

    mismo evento
    ejecutado dos veces
    =
    un único efecto de negocio

Cuando el sistema externo soporte claves de idempotencia, deberán utilizarse.

Cuando no las soporte, Agro Ops deberá mantener referencias externas y controles locales que permitan detectar duplicados.

## Retries

Los errores temporales deberán poder reintentarse.

Ejemplos:

- timeout;
- HTTP 503;
- SOAP fault temporal;
- conexión rechazada;
- token expirado.

Los retries deberán utilizar una política de backoff.

Los errores permanentes no deberán reintentarse indefinidamente.

Ejemplos:

- mapeo inexistente;
- documento inválido;
- entidad externa inexistente;
- configuración incorrecta.

Estos casos deberán quedar visibles para intervención.

## Transacciones

La API no deberá confirmar una operación crítica y luego intentar insertar el outbox fuera de la transacción.

Incorrecto:

    guardar negocio
    COMMIT

    insertar outbox

Correcto:

    BEGIN

    guardar negocio
    guardar audit
    guardar outbox

    COMMIT

De esta forma:

- o existen todos los efectos locales;
- o no existe ninguno.

## Relación con sistemas externos

Los adapters de ARCA y Finnegans serán ejecutados por workers o jobs cuando el flujo sea asíncrono.

No se deberán introducir llamadas externas frágiles dentro de una transacción PostgreSQL de negocio.

## PostgreSQL

PostgreSQL será utilizado inicialmente para:

- datos de negocio;
- outbox;
- jobs;
- locks;
- estado de retries.

No se introducirá Redis, Kafka, RabbitMQ u otro message broker en V1 salvo que exista una necesidad demostrada.

## Concurrencia

Dos workers no deberán procesar simultáneamente el mismo evento.

La implementación deberá utilizar mecanismos seguros de PostgreSQL, por ejemplo locking transaccional apropiado.

La estrategia exacta se definirá durante implementación y deberá estar cubierta por tests de concurrencia.

## Observabilidad

Cada evento o job deberá permitir conocer:

- cuándo fue creado;
- cuándo se intentó procesar;
- cuántos intentos tuvo;
- cuál fue el último error;
- cuándo se completó;
- qué sistema externo estuvo involucrado.

Los errores no deberán desaparecer únicamente en logs.

## Reglas

- Cambio de negocio y outbox se confirman juntos.
- La API no depende de que ARCA o Finnegans estén disponibles para confirmar una operación local cuando el flujo pueda ser asíncrono.
- Los retries deben ser idempotentes.
- El worker puede reiniciarse sin perder trabajo.
- Los jobs deben persistir su estado.
- Un evento fallido debe poder investigarse.
- No introducir un message broker externo sin necesidad demostrada.
- Los sistemas externos deben permanecer detrás de adapters.

## Pruebas obligatorias

La implementación deberá demostrar mediante tests:

- operación de negocio y outbox se crean en la misma transacción;
- rollback elimina ambos efectos;
- retry del mismo evento no duplica efectos;
- dos workers no procesan simultáneamente el mismo evento;
- reiniciar worker no pierde eventos;
- error temporal programa nuevo intento;
- error permanente puede llegar a DEAD;
- evento exitoso no vuelve a procesarse;
- caída del sistema externo no pierde la operación local ya confirmada.

## Consecuencias

### Ventajas

- Integraciones más confiables.
- Reintentos seguros.
- Menor acoplamiento con sistemas externos.
- Mejor tolerancia a fallos.
- Auditoría del procesamiento.
- Permite mantener la API rápida.
- Evita depender de ARCA o Finnegans durante una request de usuario.

### Costos

- Introduce procesamiento eventual.
- Algunas operaciones no se reflejarán inmediatamente en sistemas externos.
- Requiere UI de estado y monitoreo.
- Deben diseñarse correctamente retries e idempotencia.
- Aumenta la cantidad de estados operativos a gestionar.

## Alternativas consideradas

### Llamadas externas sincrónicas dentro de la request

No se adopta como estrategia general debido a la fragilidad frente a fallas parciales.

### Redis

No se considera necesario inicialmente.

### RabbitMQ

No se considera necesario inicialmente.

### Kafka

No se considera necesario para la escala prevista de V1.

Si el volumen o los requisitos futuros lo justifican, la arquitectura podrá evolucionar mediante un nuevo ADR.

## Regla

El procesamiento asíncrono crítico de V1 utilizará PostgreSQL como mecanismo persistente para outbox y jobs.

Introducir un message broker externo requiere una decisión arquitectónica explícita documentada mediante ADR.
