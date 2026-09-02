# ADR-009: Integración ARCA/CPE V1 read-only

## Estado

Aceptado

## Contexto

La Carta de Porte Electrónica es un documento legal administrado por ARCA.

Agro Ops necesita conocer y relacionar CPE con su operación para:

- trazabilidad de granos;
- relación con contratos;
- relación con campañas;
- relación con establecimientos y lotes;
- conciliación de kilos;
- seguimiento de estados;
- comercialización.

Implementar emisión o modificación de CPE desde Agro Ops en V1 aumentaría significativamente la criticidad legal y operativa del sistema.

El objetivo inicial es obtener valor operativo sin asumir responsabilidad de emisión legal.

## Decisión

La integración ARCA/CPE de V1 será read-only.

Agro Ops podrá:

- autenticarse contra ARCA;
- consultar CPE;
- importar CPE;
- actualizar información de CPE ya importadas;
- almacenar referencias legales;
- relacionar CPE con entidades internas;
- almacenar documentación asociada cuando corresponda.

Agro Ops no podrá en V1:

- emitir CPE;
- modificar CPE;
- anular CPE;
- reemplazar a ARCA como fuente legal.

## Fuente de verdad

ARCA será la fuente de verdad legal de CPE y CTG.

Agro Ops mantendrá una réplica operativa.

Conceptualmente:

    ARCA
      |
      | CPE legal
      v
    ARCA Adapter
      |
      v
    Agro Ops
      |
      +-- relaciones con contrato
      +-- campaña
      +-- establecimiento
      +-- stock
      +-- certificado
      +-- liquidación

Los datos legales no deberán modificarse localmente de manera que contradigan a ARCA.

## Integración técnica

La integración quedará aislada detrás del módulo/adaptador ARCA.

El dominio no dependerá de:

- SOAP;
- XML;
- WSAA;
- WSCPE;
- certificados digitales;
- tokens ARCA.

El adapter será responsable de traducir entre el modelo externo y el modelo interno.

## Autenticación

La autenticación utilizará los mecanismos oficiales requeridos por ARCA.

Esto incluirá, según corresponda:

- WSAA;
- certificado;
- private key;
- Ticket de Acceso;
- renovación de credenciales.

Los certificados y claves privadas serán secretos de infraestructura.

Nunca se commitearán al repositorio.

## CPE interna

Cada CPE importada deberá conservar referencias suficientes para garantizar trazabilidad e idempotencia.

Como mínimo, el modelo deberá poder representar información equivalente a:

- identificador externo;
- CTG cuando corresponda;
- estado;
- fechas;
- producto;
- kilos;
- origen;
- destino;
- transportista;
- referencias relevantes;
- fecha de última sincronización.

El schema exacto se definirá durante implementación.

## Idempotencia

Importar la misma CPE varias veces no deberá crear múltiples entidades.

Conceptualmente:

    importar CPE 123
    importar CPE 123
    importar CPE 123

debe producir:

    una CPE interna

con actualizaciones de estado o versión cuando corresponda.

## Actualizaciones

Una CPE ya importada puede cambiar de estado en ARCA.

La sincronización deberá detectar cambios y actualizar la réplica operativa sin perder historia relevante.

Cuando sea necesario, se registrarán:

- versión previa;
- nueva versión;
- fecha de sincronización;
- audit event.

## Descubrimiento

Agro Ops deberá poder descubrir nuevas CPE mediante jobs programados.

Conceptualmente:

    cron
      |
      v
    job sync-arca
      |
      v
    ARCA Adapter
      |
      v
    nuevas CPE
      |
      v
    PostgreSQL

La frecuencia exacta se definirá durante el POC.

## Bandeja de pendientes

Cuando una CPE no pueda relacionarse automáticamente con seguridad, deberá quedar disponible en una bandeja de pendientes.

Ejemplos de relaciones manuales:

- establecimiento;
- campaña;
- contrato;
- lote o unidad operativa;
- contraparte.

El sistema no deberá inferir relaciones críticas cuando exista ambigüedad.

## Relación con Finnegans

Finnegans no será el intermediario obligatorio para obtener CPE de ARCA.

Agro Ops podrá consultar directamente ARCA mediante su adapter.

Finnegans podrá conservar las referencias necesarias para su circuito contable/fiscal.

Cuando sea necesario compartir relaciones operativas con Finnegans, se realizará mediante el adapter Finnegans.

## Documentos

Cuando ARCA permita obtener documentación asociada, Agro Ops podrá almacenarla en Supabase Storage.

La metadata deberá conservar información como:

- CPE relacionada;
- storage path;
- tipo de documento;
- hash;
- fecha de obtención.

## Errores y retries

Errores temporales deberán procesarse mediante jobs/outbox y retries.

Ejemplos:

- timeout;
- servicio no disponible;
- Ticket de Acceso expirado;
- problemas de red;
- error temporal SOAP.

Los errores permanentes deberán quedar visibles para intervención.

## Pruebas obligatorias

La implementación deberá cubrir:

- autenticación exitosa;
- credencial expirada;
- timeout;
- SOAP fault;
- XML inválido o incompleto;
- importación repetida de la misma CPE;
- cambio de estado de una CPE existente;
- worker reiniciado durante sincronización;
- CPE sin relación interna;
- CPE relacionada manualmente;
- ejecución repetida del job sin duplicados.

Se utilizarán fixtures anonimizadas de respuestas reales cuando sea posible.

## Homologación

Antes de producción deberá existir un POC contra el ambiente de homologación o mecanismo oficial disponible de ARCA.

Las pruebas contra servicios externos reales no deberán bloquear necesariamente cada push de CI.

Podrán ejecutarse como:

- nightly;
- manual;
- milestone.

## Consecuencias

### Ventajas

- Reduce riesgo legal y operativo en V1.
- Permite obtener trazabilidad real de CPE.
- Agro Ops puede construir el modelo comercial sin depender de Finnegans como intermediario.
- Simplifica la primera integración con ARCA.
- Permite validar autenticación, parsing, retries e idempotencia antes de emitir documentos.

### Costos

- Los usuarios continuarán utilizando el mecanismo vigente para emitir CPE.
- Agro Ops no será inicialmente un punto único para todas las operaciones de Carta de Porte.
- Puede existir una demora entre un cambio en ARCA y su réplica en Agro Ops.

## Evolución futura

La emisión o modificación de CPE desde Agro Ops podrá evaluarse después de estabilizar:

- modelo operativo;
- integración read-only;
- seguridad;
- auditoría;
- conciliación;
- procesos de soporte.

Esa capacidad requerirá un nuevo ADR.

## Regla

En V1, ARCA es la fuente legal de CPE y Agro Ops consume esa información en modo read-only.

No implementar emisión, modificación o anulación de CPE sin una nueva decisión arquitectónica explícita.
