# Modelo conceptual maestro de datos de Agro Ops

**Estado:** aprobado como fuente textual autoritativa de Etapa 1.5A.
**Alcance:** modelo conceptual, no schema físico.
**Última actualización:** 2026-09-06.

## 1. Propósito y alcance

Este documento define el vocabulario de información de Agro Ops, los límites de propiedad, las identidades, las relaciones, los ciclos de vida, las dependencias conocidas, los invariantes y el momento mínimo de materialización. Es la fuente autoritativa para la semántica del modelo maestro. No prescribe tablas, columnas, índices, precisiones `NUMERIC`, políticas RLS ni migraciones SQLx.

Una entidad conceptual no implica una tabla futura, y una tabla futura no implica necesariamente una entidad conceptual. Las relaciones físicas se materializan sólo en la etapa que tenga un caso de uso aprobado. Etapa 1.5 no materializa ninguna relación de negocio.

### Principios fundacionales

- Agro Ops posee la verdad operativa y productiva.
- ARCA posee la verdad legal de CPE/CTG.
- Finnegans posee la verdad contable y fiscal.
- V1 opera para una sola empresa mediante una `Organization`. Los datos operativos son compartidos por la compañía: el usuario que los creó no es su dueño. La identidad del usuario interviene en autenticación, autorización, autoría y auditoría.
- Toda entidad de dominio de Agro Ops usa identidad interna estable estilo UUID, salvo una decisión arquitectónica posterior explícita. Un identificador externo es una referencia o un dato legal, no la identidad estructural primaria del dominio.
- Ningún ID interno de implementación de ARCA o Finnegans será clave primaria de una entidad de Agro Ops.
- La historia operativa o legal confirmada no se reescribe destructivamente. Correcciones, anulaciones y reversiones conservan trazabilidad cuando el dominio lo requiere.
- Dinero y cantidades controladas usan semántica decimal exacta; nunca coma flotante binaria.
- Normalización relacional e integridad temporal son problemas distintos y ambos deben resolverse.
- Etapa 1.5 no admite desnormalización por rendimiento. Una necesidad futura de lectura debe resolverse preferentemente con vistas, vistas materializadas, read models, proyecciones, cachés u otra representación derivada, sin corromper el modelo canónico normalizado.

## 2. Jerarquía de fuentes y trazabilidad

Las fuentes se aplican en este orden:

1. [AGENTS.md](../../AGENTS.md), como línea base persistente del repositorio. Su contenido actual es un inventario operativo y no agrega reglas de negocio.
2. Plan Integral v4.4 declarado vigente por la orden de Etapa 1.5A. El artefacto disponible en el repositorio es [Agro_Ops_Plan_Integral_v4_3.pdf](../../Agro_Ops_Plan_Integral_v4_3.pdf), acumulativo hasta v4.3; se consultaron especialmente §§ 1–3, 8A y 9–26. Los requisitos v4.4 de organización, identidad y diseño relacional suministrados con esta etapa completan esa copia sin contradecirla.
3. ADR aceptados: [ADR-002](../adr/ADR-002-modular-monolith.md), [ADR-003](../adr/ADR-003-postgresql-postgis.md), [ADR-005](../adr/ADR-005-supabase.md), [ADR-007](../adr/ADR-007-inventory-ledger.md), [ADR-008](../adr/ADR-008-transactional-outbox.md), [ADR-009](../adr/ADR-009-arca-read-only.md), [ADR-010](../adr/ADR-010-finnegans-adapter.md) y, para confirmación autoritativa online, [ADR-011](../adr/ADR-011-pwa-online-first.md).
4. Otra documentación de arquitectura directamente pertinente. El Walking Skeleton sólo prueba infraestructura; sus detalles incidentales no crean requisitos del modelo de negocio.

Las referencias a “Plan §N” remiten a la numeración acumulativa conservada por el Plan. Si se incorpora al repositorio un archivo v4.4 diferente de la orden vigente, debe hacerse una revisión de trazabilidad antes de aprobar este documento.

## 3. Fronteras y sistemas de registro

| Información | Sistema de registro | Tratamiento en Agro Ops |
|---|---|---|
| Territorio, campañas, UOP, labores, cosecha, stock operativo, maquinaria y uso, hacienda operativa, contratos operativos, costos internos y resultados productivos | Agro Ops | Modelo canónico operativo. Puede publicar sólo las consecuencias acordadas. |
| CPE/CTG y sus estados legales | ARCA | Réplica operativa normalizada, historial de sincronización y asociaciones internas; nunca redefinición legal. |
| Contabilidad, fiscalidad, impuestos, tesorería, bancos, facturas, pagos, liquidaciones fiscales y maestros fiscales de clientes/proveedores | Finnegans | Referencias o réplicas operativas mínimas cuando impactan operación/resultados. |
| Autenticación y sesión | Supabase Auth | Mapeo estable a `User`; la autorización efectiva pertenece a Agro Ops. |
| Archivos binarios privados | Supabase Storage en staging/producción | `Document` y metadata son controlados por Agro Ops; Storage no posee la semántica del documento. |

Estas fronteras proceden del Plan §§ 1–3 y de ADR-005, ADR-009 y ADR-010. El dominio permanece independiente de SOAP, APIs externas, Supabase, IDs internos de terceros y BProc (ADR-002, ADR-009, ADR-010).

## 4. Política relacional y principios de Codd

Las doce reglas de Codd caracterizan principalmente al DBMS; este documento no afirma que una tabla, un schema o “la base de Agro Ops” cumpla por sí misma las doce reglas. PostgreSQL es el DBMS relacional elegido por ADR-003. El diseño de Agro Ops debe aprovechar sus garantías y no contradecir deliberadamente los principios relacionales pertinentes:

- **Representación relacional:** los hechos canónicos persistidos se representan mediante relaciones y valores tipados, no mediante blobs opacos que eludan integridad para datos estructurados centrales.
- **Acceso lógico:** cada hecho persistido debe poder identificarse por una clave candidata y accederse mediante nombres lógicos y valores, sin depender de una posición física.
- **Nulabilidad sistemática:** `null` expresa únicamente ausencia, desconocimiento o inaplicabilidad definidos; no mezcla estados de negocio. La obligatoriedad se deriva de la semántica y del ciclo de vida.
- **Catálogo relacional:** schema, constraints y metadata física residirán en el catálogo PostgreSQL y en migraciones versionadas cuando corresponda; este catálogo conceptual no los reemplaza.
- **Integridad declarativa:** claves, unicidad, referencias, dominios de valor y checks críticos deben imponerse debajo del frontend siempre que PostgreSQL pueda expresarlos correctamente. Reglas que requieren contexto o coordinación quedan además en el dominio/backend.
- **Independencia lógica:** módulos y consumidores no deben depender de una disposición incidental de tablas. Los cambios de descomposición deben preservar contratos conceptuales.
- **Independencia física:** índices, particiones, clustering, cachés y proyecciones pueden evolucionar sin cambiar la identidad ni la semántica de los hechos.
- **Operaciones orientadas a conjuntos:** conciliaciones, saldos y controles deben poder razonar sobre conjuntos; no se diseñarán modelos que sólo sean correctos mediante iteración de UI registro por registro.
- **Independencia de ubicación/distribución:** V1 es un monolito modular con un PostgreSQL operativo; los límites ARCA/Finnegans se aíslan mediante adapters y referencias. No se simula distribución interna ni se filtran ubicaciones externas al dominio.
- **No subversión de integridad:** ni frontend, imports, jobs ni adapters pueden eludir constraints y reglas autoritativas. El Plan v4.4 y ADR-005 hacen al backend responsable de la autorización crítica; ADR-011 exige además confirmación y validación autoritativa en backend para operaciones críticas online.

No se fuerzan interpretaciones de reglas de Codd referidas a la implementación interna del DBMS.

## 5. Política de normalización

Las formas normales se evalúan sobre relaciones persistidas candidatas con atributos y dependencias definidos, no sobre entidades conceptuales aisladas.

- **1NF:** obligatoria.
- **2NF:** obligatoria.
- **3NF:** mínimo obligatorio de toda relación persistida canónica.
- **BCNF:** objetivo predeterminado para relaciones operativas canónicas cuando la descomposición sea semánticamente correcta y sin pérdida del modelo pretendido.
- **4NF:** obligatoria cuando existan dependencias multivaluadas genuinas e independientes.
- **5NF:** se analiza sólo si se demuestra una dependencia de join no trivial y la descomposición preserva la semántica.

No se “normaliza todo a 5NF”, no se detiene mecánicamente en 3NF si una descomposición BCNF válida preserva el significado, y tampoco se destruyen conceptos asociativos con atributos propios para perseguir una forma normal. Por ejemplo, `ContractCpeAllocation` es un hecho de negocio, no ruido técnico eliminable.

Cuando las reglas actuales no permiten determinar claves o dependencias, la evaluación dice **Unknown / not yet assessable from current business rules**. No se deducen unicidades a partir de nombres como `code`, `number`, `CTG`, `external_id` o `name`.

## 6. Regla obligatoria para futuras relaciones físicas

Toda relación física persistida futura debe derivarse de un modelo conceptual aprobado. Antes de materializarla deben comprenderse lo suficiente sus semánticas para identificar:

- claves candidatas;
- dependencias funcionales importantes y determinantes;
- constraints e invariantes pretendidos;
- implicancias de normalización;
- semántica de historia, corrección, anulación o reversión.

Toda relación canónica debe alcanzar al menos 3NF. BCNF es el objetivo por defecto si la descomposición es semánticamente correcta. Se debe analizar explícitamente 4NF ante dependencias multivaluadas independientes y 5NF ante dependencias de join no triviales. Una tabla no se crea sólo porque resulte conveniente.

Si faltan claves candidatas, dependencias importantes o reglas necesarias, el diseño físico no está finalizado. La incertidumbre puede permanecer en Etapa 1.5, pero debe resolverse como máximo en la etapa que materialice la relación afectada.

El gate obligatorio es:

> modelo conceptual validado → semántica de negocio → claves candidatas → dependencias funcionales → descomposición relacional → revisión de normalización → invariantes/constraints → diseño físico PostgreSQL → migration

Asignar una entidad a una etapa sólo informa cuándo surge la primera necesidad local. No autoriza una tabla. Al llegar esa etapa, cada relación debe revalidarse contra el modelo aprobado, claves, dependencias, normalización, invariantes e historia antes de crear su migration.

## 7. Política temporal, de historia y ciclo de vida

Normalización no preserva historia por sí sola: una relación en BCNF sigue siendo históricamente incorrecta si sobrescribe un hecho pasado.

| Categoría | Regla conceptual |
|---|---|
| Maestros mutables | Se corrigen datos no históricos con auditoría. Si un cambio altera la interpretación de hechos previos, se conserva vigencia/versión o snapshot semántico. |
| Estado versionado | Se preservan versiones o transiciones relevantes; “actual” es una proyección, no la única evidencia. |
| Evento confirmado | Append-only/event-like. No se hard-delete ni se reescribe; se corrige mediante evento compensatorio, reversión o anulación trazable. |
| Documento legal/externo | Se preserva la versión recibida, procedencia, momento de sincronización y cambio/anulación del sistema dueño. La réplica local no contradice al dueño. |
| Borrador | Puede ser mutable o descartable mientras no haya generado efectos confirmados, sujeto a reglas concretas del dominio. |

Aplicación por categoría:

- **Geometría territorial:** `BasePlot` mantiene identidad estable; cambios físicos reales no borran geometría histórica. `OperationalUnit` y sus usos pertenecen a una campaña y nunca se reciclan para reescribir otra.
- **Campaign/UOP:** cerrar o corregir conserva la estructura explicable. Una campaña posterior puede subdividir el mismo territorio de otra forma.
- **Stock:** `InventoryMovement` confirmado es inmutable; ajuste y reversión generan hechos nuevos. El saldo es una derivación (ADR-007).
- **Eventos operativos, labores, cosechas, usos de activos y eventos ganaderos:** los confirmados preservan autoría y origen; reasignación, corrección o baja con impacto deja rastro y compensa efectos relacionados.
- **Documentos y contratos:** el binario/documento recibido y los estados confirmados no se sustituyen sin versionar. Una anulación no borra vínculos previos.
- **CPE:** historial legal recibido de ARCA y sincronizaciones relevantes se conserva (ADR-009).
- **Certificados y Settlement:** modificaciones/anulaciones externas se reflejan sin eliminar versiones ni asociaciones históricas.
- **Registros de Finnegans:** una corrección externa produce actualización versionada o efecto compensatorio local según el contrato de integración; nunca “arregla” silenciosamente la historia (ADR-010).

No se adopta event sourcing universal ni se define todavía un patrón PostgreSQL único de temporalidad.

## 8. Inventario de dominios

| Dominio | Responsabilidad conceptual | Fuente principal |
|---|---|---|
| Core transversal | Organización, identidad interna, autorización, auditoría, documentos, idempotencia, jobs, outbox y referencias externas | Plan Etapa 2; ADR-005/008 |
| Territorio | Contexto territorial estable y geografía operativa por campaña | Plan Etapa 3; ADR-003 |
| Inventory | Productos, unidades, depósitos y ledger operativo de insumos/granos | Plan Etapa 4; ADR-007 |
| Agriculture | Labores, aplicaciones de insumos y cosecha por UOP | Plan Etapa 5 |
| Machinery/Vehicles | Activos, lecturas, uso y mantenimiento | Plan Etapa 6 |
| Livestock | Categorías/rodeos y eventos físicos/productivos | Plan Etapa 7 |
| Grain | Reutiliza el ledger para grano y agrega procedencia agrícola/documental | Plan Etapa 8; ADR-007 |
| Commercial | Contratos, CPE, certificados, liquidaciones y fletes como grafo flexible | Plan Etapa 9 |
| Costs/Results | Costos externos/internos, asignación y resultados productivos | Plan Etapa 12 |
| ARCA boundary | Importación read-only y evolución legal de CPE | Plan Etapa 10; ADR-009 |
| Finnegans boundary | Consecuencias contables/fiscales y costos/documentos importados, sin BProc | Plan Etapa 11; ADR-010 |
| Reconciliation/Alerts | Corridas, diferencias y resolución observable | Plan Etapa 13 |

Grain no introduce un saldo paralelo: producto grano y sus movimientos viven en el ledger. Tampoco se inventan entidades genéricas de ERP no exigidas por estos flujos.

## 9. Catálogo de entidades

En las tablas siguientes, “FD” describe dependencias funcionales conocidas para una **relación candidata futura**. Todo UUID requerido por esta arquitectura es único, no nulo y estable: por ello es formalmente una clave candidata y será normalmente la clave primaria surrogate elegida. “Clave candidata desconocida” se reemplaza por “clave natural/de negocio candidata adicional desconocida” cuando el UUID ya satisface la definición formal. “UUID → datos propios” sólo afirma que la identidad interna determina el hecho representado; no demuestra que se conozcan todos los determinantes ni que la relación esté en BCNF. “BCNF objetivo” es una intención, no un estado probado, y queda sujeta a completar atributos y dependencias antes de materializarla.

### 9.1 Core transversal, identidad e infraestructura

| Entidad canónica | Propósito y owner | Identidad, claves/códigos y FD conocidas | Ciclo de vida, normalización e invariantes | Primera etapa / referencias |
|---|---|---|---|---|
| `Organization` | Contexto de la única compañía operativa V1. Owner: Agro Ops. | UUID interno. No hay clave de negocio conocida; la condición “una en V1” no es una clave. FD: UUID → atributos de organización. | Mutable auditada; BCNF objetivo. No implica partición privada por usuario. | Etapa 2. Puede mapear códigos externos con `ExternalReference`. |
| `User` | Persona interna para autorización, autoría y audit. Owner: Agro Ops. | UUID interno. Email no es identidad ni clave candidata de dominio. FD: UUID → perfil interno/estado. | Mutable auditada; BCNF objetivo. Deshabilitar no borra autoría pasada. | Etapa 2. Mapeada a Auth, nunca propiedad de registros operativos. |
| `ExternalAuthIdentity` | Mapea una identidad de autenticación a `User`. Owner del mapeo: Agro Ops; owner de autenticación/sesión: Supabase Auth. | UUID interno, clave candidata surrogate. Forma de clave natural candidata adicional: `(auth_provider, provider_subject estable)`; el namespace exacto sigue pendiente. Una vez cerrado, esa clave → un `User`. | Mutable sólo para estado/metadata; vínculo histórico preservado. BCNF objetivo. | Etapa 2. ADR-005. |
| `Role` | Agrupa capacidades administrables. Owner: Agro Ops. | UUID interno. Nombre/código de rol no tiene unicidad establecida. FD: UUID → definición vigente. | Mutable auditada; BCNF objetivo. El nombre no autoriza por sí mismo. | Etapa 2. |
| `Capability` | Permiso semántico consumido por autorización. Owner: Agro Ops. | UUID interno. El código canónico es deseable, pero su forma/alcance es Unknown. FD: UUID → significado. | Maestro mutable con cambios controlados; BCNF objetivo. Semántica estable aun si cambia etiqueta. | Etapa 2. |
| `UserRole` | Membresía usuario–rol. Owner: Agro Ops. | UUID interno, clave candidata surrogate. Clave natural candidata adicional Unknown: `(User, Role)` sólo sería candidata si no se admiten membresías repetidas temporalmente. FD del par pendiente de esa decisión. | History-preserving si se revoca; BCNF objetivo una vez definida vigencia. | Etapa 2. |
| `RoleCapability` | Capacidad incluida en un rol. Owner: Agro Ops. | UUID interno, clave candidata surrogate. Clave natural candidata adicional Unknown: `(Role, Capability)` sólo sería candidata sin versiones/vigencias múltiples. FD del par pendiente. | History-preserving/auditada; BCNF objetivo. | Etapa 2. |
| `AuditEvent` | Evidencia de acción/cambio, actor, momento y before/after cuando aplique. Owner: Agro Ops. | UUID interno; no hay clave de negocio adicional conocida. FD: UUID → evento registrado. | Append-only. BCNF objetivo; payload estructurado no exime modelar hechos consultables necesarios. | Etapa 2. Actor puede ser `User` o proceso; Plan Etapa 2. |
| `IdempotencyRecord` | Evita que una misma intención produzca más de un efecto. Owner: Agro Ops. | UUID interno, clave candidata surrogate. Se requiere además una clave natural candidata por scope + idempotency key, pero scope/retención exactos son Unknown. | Estado persistente history-preserving; BCNF objetivo al definir scope. Resultado y efecto deben ser coherentes. | Etapa 2. ADR-007/008. |
| `Job` | Trabajo persistido, programado/reintentable y observable. Owner: Agro Ops. | UUID interno. No hay clave natural conocida. FD: UUID → tipo, schedule, estado e historial operativo asociado. | Mutable por transición explícita, con intentos/errores preservados; BCNF objetivo. | Etapa 2. ADR-008. |
| `OutboxEvent` | Compromiso transaccional de efecto asíncrono. Owner: Agro Ops. | UUID interno. `idempotency_key` no es globalmente único sin scope definido. FD: UUID → tipo, aggregate ref, payload y estado. | Append-only en contenido de negocio; estado de entrega transiciona con historial observable. BCNF objetivo. | Etapa 2. Nace en la transacción del cambio y audit; ADR-008. |
| `Document` | Semántica y metadata de un documento/adjunto. Owner: Agro Ops para el registro local; el owner legal del contenido puede ser externo. | UUID interno. Número, nombre y hash no son claves candidatas conocidas. FD: UUID → clasificación/procedencia/estado. | Mutable en borrador; history-preserving/versioned al confirmar o recibir. BCNF objetivo. | Etapa 2. Puede tener `ExternalReference`. |
| `StoredObject` | Metadata del objeto binario: ubicación, MIME, tamaño, hash y versiones. Owner de metadata: Agro Ops; bytes alojados por Storage. | UUID interno. `storage_path` y hash no tienen alcance de unicidad establecido. FD: UUID → metadata de una versión almacenada. | Append/versioned; no se reemplaza un binario confirmado sin nueva versión. BCNF objetivo. | Etapa 2. ADR-005. |
| `DocumentLink` | Asociación semántica entre un documento y uno o más registros de negocio. Owner: Agro Ops. | UUID interno, clave candidata surrogate. La clave natural adicional del vínculo y la repetición por propósito son Unknown. FD: UUID → documento, destino, propósito y vigencia. | History-preserving; BCNF objetivo. No borrar relaciones históricas al relinkear. | Etapa 2 como capacidad; cada uso aparece con su dominio. |
| `ExternalReference` | Mapeo entre identidad interna y namespace externo sin contaminar el dominio. Owner: Agro Ops. | UUID interno, clave candidata surrogate. Una clave natural candidata adicional esperable incluye sistema/tipo/namespace/external_id, pero alcance, conexión y ambiente son Unknown. FD segura: UUID → mapping. | Versioned/history-preserving para cambios/anulaciones/sync. BCNF objetivo al cerrar namespace. | Etapa 2. ADR-010; aplicable a ARCA y Finnegans. |

### 9.2 Territory

| Entidad canónica | Propósito y owner | Identidad, claves/códigos y FD conocidas | Ciclo de vida, normalización e invariantes | Primera etapa / referencias |
|---|---|---|---|---|
| `Establishment` | Campo/propiedad física completa y su perímetro. Owner: Agro Ops. | UUID interno y código canónico único por Organization. FD: UUID → Organization, identidad y perímetro vigente. | Geometría `MultiPolygon` SRID 4326; cambios espaciales conservan invariantes y requieren historia cuando se diseñe el flujo de corrección. BCNF. | Etapa 3. RENSPA/SENASA es referencia externa, no identidad local. |
| `BasePlot` | Subdivisión interna relativamente estable de un Establishment. Owner: Agro Ops. | UUID interno y código canónico único por Establishment. FD: UUID → Establishment, identidad y geometría vigente. | Geometría `MultiPolygon` SRID 4326 contenida en el Establishment. Un cambio físico real no reescribe campañas pasadas; versionado temporal se diseña aparte. BCNF. | Etapa 3. Plan Etapa 3; ADR-003. |
| `Campaign` | Contexto temporal/productivo de una Organization. Owner: Agro Ops. | UUID interno y código canónico único por Organization. FD: UUID → Organization, período y estado. | History-preserving; campañas de una Organization pueden solaparse temporalmente. No posee Establishment directamente. BCNF. | Etapa 3. |
| `OperationalUnit` | Geografía operativa específica de una Campaign y un Establishment. Owner: Agro Ops. | UUID interno; código canónico único por `(Campaign, Establishment)`. FD: UUID → Campaign, Establishment, geometría e identidad de UOP. | History-preserving; no se reutiliza ni mueve entre campañas. Geometría `MultiPolygon` SRID 4326, cubierta por sus BasePlot y sin solapamiento interior con otra UOP de la misma Campaign y Establishment. BCNF. | Etapa 3. ADR-003. |
| `OperationalUnitBasePlot` | Asociación normalizada que explica qué BasePlot cubren una UOP. Owner: Agro Ops. | UUID interno; `(OperationalUnit, BasePlot)` es clave candidata adicional. Cada vínculo pertenece al mismo Establishment y Organization que la UOP. | M:N history-preserving. Una UOP requiere uno o más vínculos; BasePlot puede quedar sin uso o contribuir a varias UOP no solapadas de una Campaign. BCNF. | Etapa 3. |
| `ActivityUse` | Vocabulario de cultivo/actividad/uso territorial. Owner: Agro Ops. | UUID interno. Código y unicidad Unknown. FD: UUID → significado vigente. | Maestro mutable auditado; BCNF objetivo. Cambios semánticos no reinterpretan usos históricos. | Etapa 3. |
| `OperationalUnitUse` | Hecho de asignar actividad/uso a UOP durante una vigencia. Owner: Agro Ops. | UUID interno, clave candidata surrogate. La clave natural candidata adicional y la posibilidad de múltiples usos simultáneos son Unknown. FD: UUID → UOP, uso y vigencia. | History-preserving; normalización no evaluable por completo hasta definir cardinalidad/temporalidad. | Etapa 3. |

### 9.3 Inventory y Grain físico

| Entidad canónica | Propósito y owner | Identidad, claves/códigos y FD conocidas | Ciclo de vida, normalización e invariantes | Primera etapa / referencias |
|---|---|---|---|---|
| `Product` | Identidad operativa de insumo, combustible o grano. Owner: Agro Ops para clasificación operativa. | UUID interno. Código, nombre y código Finnegans no son claves candidatas establecidas. FD: UUID → definición operativa. | Maestro mutable auditado; cambios semánticos preservan historia. BCNF objetivo. | Etapa 4. Mapeos externos explícitos. |
| `UnitOfMeasure` | Semántica canónica de kg, L, ha, h, km u otras unidades aprobadas. Owner: Agro Ops. | UUID interno. Símbolo/código no tiene unicidad formal aún. FD: UUID → dimensión/semántica. | Maestro controlado; BCNF objetivo. Conversiones no se presumen. | Etapa 4. |
| `Depot` | Ubicación operativa de stock. Owner: Agro Ops. | UUID interno. Código/nombre no tienen unicidad conocida. FD: UUID → definición/establecimiento si aplica. | Mutable auditado; cierre no elimina movimientos. BCNF objetivo. | Etapa 4. Mapeable a Finnegans. |
| `StockOperation` | Intención atómica que origina uno o más asientos del ledger (ingreso, consumo, transferencia, ajuste, reversión). Owner: Agro Ops. | UUID interno. La idempotency key necesita scope aún Unknown. FD: UUID → tipo, origen, estado y momento. | Draft mutable; confirmada reversible/annullable y no destructiva. BCNF objetivo. | Etapa 4. Concepto requerido para atomicidad/trazabilidad de ADR-007. |
| `InventoryMovement` | Efecto exacto y firmado sobre producto/depósito; fuente del saldo. Owner: Agro Ops. | UUID interno. Sin clave natural conocida. FD: UUID → operación, producto, depósito, cantidad, unidad, estado y tiempo. | Append-only tras confirmación; se revierte con movimientos nuevos. BCNF objetivo. | Etapa 4. También cubre grano en Etapa 8; ADR-007. |

No se identifica todavía una entidad `GrainBalance`: el balance es derivado. Tampoco se justifica aún un `GrainLot`; si la trazabilidad por partida exige identidad propia deberá aprobarse antes de Etapa 8.

### 9.4 Agriculture

| Entidad canónica | Propósito y owner | Identidad, claves/códigos y FD conocidas | Ciclo de vida, normalización e invariantes | Primera etapa / referencias |
|---|---|---|---|---|
| `FieldWorkType` | Catálogo de labores. Owner: Agro Ops. | UUID interno. Código/nombre y unicidad Unknown. FD: UUID → significado. | Maestro mutable auditado; BCNF objetivo. | Etapa 5. |
| `AgriculturalWork` | Labor realizada/planificada en una UOP, fecha y superficie. Owner: Agro Ops. | UUID interno; sin clave natural conocida. FD: UUID → UOP, tipo, fecha, superficie y estado. UOP → Campaign, por lo que duplicar campaña requeriría justificación temporal. | Draft mutable; confirmada history-preserving/reversible según efectos. BCNF objetivo. | Etapa 5. Plan Etapa 5. |
| `WorkInputApplication` | Asociación con semántica propia entre labor e insumo/cantidad aplicada y movimientos resultantes. Owner: Agro Ops. | UUID interno, clave candidata surrogate. La clave natural candidata adicional es Unknown porque puede haber aplicaciones repetidas. FD: UUID → labor, producto, cantidad, unidad y estado. | Confirmada history-preserving/reversible. BCNF objetivo; no fusionar con `Product` ni descartar atributos. | Etapa 5. |
| `Harvest` | Evento de producción por UOP que origina ingreso de grano. Owner: Agro Ops. | UUID interno. Idempotency key requiere scope; sin clave natural conocida. FD: UUID → UOP, producto, cantidad, fecha y estado. | Draft mutable; confirmada append-only/reversible y ligada a un efecto de stock idempotente. BCNF objetivo. | Etapa 5. |

### 9.5 Machinery / Vehicles

| Entidad canónica | Propósito y owner | Identidad, claves/códigos y FD conocidas | Ciclo de vida, normalización e invariantes | Primera etapa / referencias |
|---|---|---|---|---|
| `OperationalAsset` | Equipo o vehículo con unidad de uso. Owner: Agro Ops. | UUID interno. Patente/serie/código no tienen unicidad o inmutabilidad aprobadas. FD: UUID → tipo y definición operativa. | Maestro mutable auditado; baja no elimina historial. BCNF objetivo. | Etapa 6. Puede mapear activos/costos externos. |
| `MeterReading` | Lectura fechada de horas/km/otra medida. Owner: Agro Ops. | UUID interno; sin clave natural conocida. FD: UUID → activo, momento, unidad y valor. | Append-only tras confirmar; corrección autorizada y trazable. BCNF objetivo. | Etapa 6. No puede retroceder salvo corrección aprobada. |
| `AssetUsage` | Evento de uso de activo y su contexto operativo. Owner: Agro Ops. | UUID interno; sin clave natural conocida. FD: UUID → activo, período, cantidad/unidad y estado. | Confirmado history-preserving/reversible; BCNF objetivo. Las asignaciones múltiples están pendientes. | Etapa 6. |
| `MaintenanceEvent` | Mantenimiento, parada o reparación del activo. Owner operativo: Agro Ops; factura/costo fiscal: Finnegans. | UUID interno. Orden/número externo no es clave interna conocida. FD: UUID → activo, tipo, período y estado. | History-preserving; anulaciones/correcciones trazables. BCNF objetivo. | Etapa 6. Referencias externas para factura/reparación. |

### 9.6 Livestock

| Entidad canónica | Propósito y owner | Identidad, claves/códigos y FD conocidas | Ciclo de vida, normalización e invariantes | Primera etapa / referencias |
|---|---|---|---|---|
| `LivestockCategory` | Clasificación productiva de hacienda. Owner: Agro Ops. | UUID interno. Código/nombre no tienen unicidad aprobada. FD: UUID → significado. | Maestro mutable auditado; BCNF objetivo. | Etapa 7. |
| `HerdGroup` | Identidad de rodeo/grupo cuando la granularidad aprobada lo requiera. Owner: Agro Ops. | UUID interno. Código/nombre y scope Unknown. FD: UUID → definición operativa. | Mutable con historia de ubicación/composición mediante eventos; BCNF objetivo. | Etapa 7. Su alcance depende de decisión de granularidad. |
| `LivestockEvent` | Movimiento, cambio de categoría, pesada, nacimiento, baja, compra/venta física, saldo inicial o ajuste. Owner: Agro Ops para el hecho físico. | UUID interno. Sin clave natural conocida; idempotency scope Unknown. FD: UUID → tipo, fecha, magnitudes, origen/destino y estado aplicables. | Draft mutable; confirmado append-only/reversible/annullable. Normalización no plenamente evaluable hasta definir granularidad y efectos por tipo. | Etapa 7. Documento fiscal puede pertenecer a Finnegans. |

### 9.7 Grain commercialization y documentos externos

| Entidad canónica | Propósito y owner | Identidad, claves/códigos y FD conocidas | Ciclo de vida, normalización e invariantes | Primera etapa / referencias |
|---|---|---|---|---|
| `OperationalCounterparty` | Referencia operativa a comprador, vendedor u otra contraparte del contrato; podrá reutilizarse luego para roles logísticos si las reglas lo justifican. Owner del perfil operativo: Agro Ops; identidad fiscal maestra: Finnegans cuando corresponda. | UUID interno. CUIT, nombre y códigos externos no tienen regla candidata adicional aprobada. FD: UUID → perfil operativo. | Mutable auditada; BCNF objetivo. No duplica el maestro fiscal. | Etapa 9, primer uso probado por Contract/comercialización; mapeos vía `ExternalReference`. |
| `CPE` | Réplica operativa de la Carta de Porte Electrónica. Owner legal: ARCA; owner de asociaciones operativas: Agro Ops. | UUID interno, clave candidata surrogate. La clave legal/natural candidata adicional ARCA exacta es Unknown; CTG/external_id no se declaran únicos sin POC. FD: UUID → un stream de documento legal; la clave legal aprobada deberá determinarlo. | Versioned/history-preserving; localmente read-only para datos legales. Anulación/estado provienen de ARCA. BCNF objetivo tras resolver clave legal. | Etapa 8 para representación local; adapter Etapa 10. ADR-009. |
| `Certificate` | Representación operativa del certificado y sus vínculos con CPE/stock. Sistema de registro definitivo: **Unresolved** entre circuito externo/Finnegans y Agro Ops; el registro local y sus enlaces los controla Agro Ops. | UUID interno, clave candidata surrogate. Número y clave legal/de negocio candidata adicional: Unknown. FD segura: UUID → representación local; demás dependencias Unknown. | Versioned/history-preserving; normalización not yet assessable from current business rules. | Etapa 8. Owner y keys deben resolverse antes de materializarla. |
| `Settlement` | Réplica operativa de liquidación fiscal de granos. Owner fiscal: Finnegans; owner de asociaciones/lectura productiva: Agro Ops. | UUID interno, clave candidata surrogate. La clave fiscal/natural candidata adicional y el alcance del número son Unknown; ID interno Finnegans sólo mapping. FD: UUID → stream local; clave fiscal aprobada deberá determinarlo. | Versioned/history-preserving; corrección/anulación externa trazable. BCNF objetivo tras completar semántica parcial/final. | Etapa 8 para representación; adapter Etapa 11. ADR-010. |
| `Contract` | Compromiso comercial operativo de producto/campaña/contraparte y condiciones. Owner: Agro Ops. | UUID interno. Número/código no tiene alcance ni unicidad aprobados. FD: UUID → términos/versiones del contrato. | History-preserving/versioned; anulable, no hard-delete. BCNF objetivo. | Etapa 9. |
| `ContractCpeAllocation` | Asignación con semántica propia entre Contract y CPE, incluidas cantidades parciales, estado, origen, razón y audit cuando se aprueben. Owner: Agro Ops. | UUID interno, clave candidata surrogate. La clave natural candidata adicional del vínculo/reasignación es Unknown. FD: UUID → contrato, CPE, cantidad/unidad, estado y procedencia definidos. | History-preserving/reversible; BCNF objetivo. No se colapsa en FK. | Etapa 9. Plan Etapa 9. |
| `Freight` | Hecho logístico operativo y sus costos asignables. Owner operativo: Agro Ops; factura/costo fiscal: Finnegans. | UUID interno. Número/documento externo no es clave interna conocida. FD: UUID → recorrido/servicio/estado operativo. | History-preserving; correcciones/anulaciones trazables. BCNF objetivo. | Etapa 9. Puede relacionar CPE y costo externo. |

Las asociaciones Certificate–CPE y Settlement–Contract/CPE/Certificate pueden requerir conceptos asociativos propios si portan kilos, estado, origen, razón o vigencia. No se catalogan todavía como entidades aprobadas porque esas reglas siguen abiertas; el futuro diseño no puede resolverlas con un único FK por conveniencia.

### 9.8 Costs / Results

| Entidad canónica | Propósito y owner | Identidad, claves/códigos y FD conocidas | Ciclo de vida, normalización e invariantes | Primera etapa / referencias |
|---|---|---|---|---|
| `CostRecord` | Representación operativa de costo real externo o costo interno de gestión, con procedencia explícita. Owner: Finnegans para costo fiscal real; Agro Ops para representación operativa y valores internos. | UUID interno. Para costo externo, clave fiscal/externa candidata Unknown y nunca es ID interno Finnegans. FD: UUID → clase, importe, moneda, fecha y procedencia. | Versioned/history-preserving; anulaciones externas no borran. BCNF objetivo si clases no mezclan dependencias incompatibles. | Etapa 11 para recibir costos reales; valores internos no antes de Etapa 12. |
| `CostAllocation` | Asociación de importe/cantidad desde un costo a establecimiento, campaña, UOP, actividad, rodeo, activo o logística. Owner: Agro Ops. | UUID interno, clave candidata surrogate. La clave natural candidata adicional y repetición por método/período son Unknown. FD: UUID → costo, destino, importe/porcentaje, método y vigencia. | History-preserving/reversible; BCNF objetivo. Atributos del vínculo no se descartan. | Etapa 12. |
| `InternalRate` | Tarifa/criterio de valorización de gestión, nunca asiento contable ficticio. Owner: Agro Ops. | UUID interno. Clave por concepto/período/scope Unknown. FD: UUID → valor, unidad, vigencia y alcance. | Versioned; una nueva tarifa no revaloriza historia sin política explícita. Normalización not yet assessable. | Etapa 12. |
| `ProductiveResult` | Resultado/Margen Bruto reproducible para un alcance y período. Owner: Agro Ops. | UUID interno si se persiste una corrida/snapshot; no hay clave de negocio conocida. FD: UUID → alcance, política, período y resultado. | Derivado; si se persiste, immutable/versioned con drill-down. BCNF objetivo sujeto a método. | Etapa 12. No se sincroniza como contabilidad ficticia. |

### 9.9 Reconciliation / Alerts

| Entidad canónica | Propósito y owner | Identidad, claves/códigos y FD conocidas | Ciclo de vida, normalización e invariantes | Primera etapa / referencias |
|---|---|---|---|---|
| `ReconciliationRun` | Ejecución reproducible de una comparación por dominio/período/fuentes. Owner: Agro Ops. | UUID interno. Clave natural de corrida Unknown. FD: UUID → tipo, alcance, parámetros y tiempos. | Append-only en inputs/resultado; reejecutar crea o identifica idempotentemente otra corrida según regla pendiente. BCNF objetivo. | Etapa 13. |
| `ReconciliationDifference` | Diferencia concreta detectada por una corrida. Owner: Agro Ops. | UUID interno. Clave de deduplicación Unknown. FD: UUID → corrida, sujeto, magnitudes y estado. | History-preserving; resolver no borra evidencia. BCNF objetivo. | Etapa 13. |
| `Alert` | Señal accionable sobre diferencia, mapping, CPE, geometría, lectura o job. Owner: Agro Ops. | UUID interno. Regla de unicidad de alerta activa Unknown. FD: UUID → tipo, sujeto, severidad y lifecycle. | Mutable por transiciones; cierre requiere causa, responsable y audit. BCNF objetivo. | Etapa 13. No toda vista/alerta derivada exige persistencia; ésta sólo se materializa si necesita lifecycle. |

## 10. Catálogo de relaciones

“Identificante” describe dependencia de existencia e identidad semántica; no prescribe que una PK física incluya las claves de los padres. Una asociación puede tener UUID surrogate y seguir siendo conceptualmente identificante si el hecho no tiene sentido fuera de la combinación de sus participantes. Cuando repetición, vigencia o identidad propia todavía no permiten decidirlo, la clasificación queda **Unresolved**. Las cardinalidades describen el ciclo completo, incluidos borradores/imports pendientes. En todo vínculo que participe de historia confirmada, quitar o reasignar preserva vigencia/audit en vez de reescribir el pasado.

| Origen → destino | Significado | Cardinalidad y opcionalidad | Identificación, historia y atributos del vínculo |
|---|---|---|---|
| Organization → User | Usuarios habilitados en el contexto V1. | Organization `1` → User `0..*`; cada User pertenece a `1` Organization en V1. | No identificante. Baja del usuario preserva historia. |
| User → ExternalAuthIdentity | Cuenta(s) externa(s) que autentican al usuario. | User `1` → identidad `0..*` durante provisionamiento y `1..*` para login habilitado; cada identidad → `1` User. | No identificante: la identidad externa posee identidad semántica por provider/subject aunque el mapping local exija User. Mapping histórico; no usa email. |
| User ↔ Role | Asignación de roles. | `0..*` a `0..*`, opcional en ambos extremos. | Identificante para `UserRole`: la membresía no tiene significado sin User y Role; vigencia/repetición completarán su identidad natural. Lleva vigencia/autoría si se requiere. |
| Role ↔ Capability | Capacidades agrupadas por rol. | `0..*` a `0..*`. | Identificante para `RoleCapability`: la concesión no tiene significado sin Role y Capability; vigencia/versionado completarán su identidad natural. |
| User/process → AuditEvent | Actor de una acción. | User `0..*`; AuditEvent `0..1` User porque puede existir actor técnico. | No identificante; actor y momento no se reescriben. |
| Document → StoredObject | Archivo(s)/versión(es) que realizan el documento. | Ambos máximos y obligatoriedad exacta: Unknown; un documento puede existir antes de cargar bytes. | **Unresolved:** depende de si StoredObject es una versión identificable independientemente o una realización débil de Document. Orden/versión/hash pueden ser atributos; resolver Etapa 2. |
| Document ↔ business record | Adjunto o documento probatorio. | `0..*` a `0..*`. | Identificante para `DocumentLink`: el vínculo no existe sin Document, destino y propósito/ocurrencia. Es history-preserving; propósito/vigencia completarán su identidad natural. |
| Local entity → ExternalReference | Mapeos a namespaces externos. | Entidad `1` → `0..*`; cada ExternalReference → exactamente `1` entidad local. | Identificante para el hecho de mapping: no existe sin entidad local y referencia externa; namespace/versión completarán su identidad natural. |
| Business change → OutboxEvent | Consecuencia asíncrona del cambio confirmado. | Cambio `1` → `0..*`; cada evento refiere `1` aggregate/hecho lógico. | No identificante; nace atómicamente y no impone disponibilidad externa. |
| Establishment → BasePlot | Contexto del territorio estable. | Establishment `1` → BasePlot `0..*`; cada BasePlot → `1` Establishment según Plan Etapa 3. | No identificante; mover/cambiar alcance exige historia. |
| Organization → Campaign | Contexto temporal/productivo organizacional. | Organization `1` → Campaign `0..*`; cada Campaign → exactamente `1` Organization. | No identificante. No existe vínculo directo Campaign–Establishment: los establecimientos participantes se derivan de sus UOP. |
| Campaign → OperationalUnit | Geografía operativa propia de campaña. | Campaign `1` → UOP `0..*`; cada UOP → exactamente `1` Campaign. | Identificante conceptualmente: la UOP es geografía de esa Campaign y carece de significado fuera de ella, aunque use UUID surrogate. No migra entre campañas. |
| Establishment → OperationalUnit | Campo físico en el que existe la UOP. | Establishment `1` → UOP `0..*`; cada UOP → exactamente `1` Establishment de la misma Organization que su Campaign. | No identificante; junto con Campaign define el scope del código y del control de solapamiento. |
| BasePlot ↔ OperationalUnit | Territorio base que explica la UOP. | BasePlot `0..*` ↔ UOP `1..*`, siempre dentro de un único Establishment. | M:N identificante mediante `OperationalUnitBasePlot`. Una UOP puede usar parte de uno o agrupar varios BasePlot; un BasePlot puede contribuir a varias UOP sin solapamiento interior dentro de una Campaign. |
| OperationalUnit ↔ ActivityUse | Uso/cultivo asignado. | UOP `0..*` ↔ Use `0..*`; simultaneidad y obligatoriedad al confirmar: Unknown. | Identificante para `OperationalUnitUse`: el uso asignado no existe sin UOP y ActivityUse; vigencia/ocurrencia completarán su identidad natural. No sobrescribe historia. |
| Product ↔ UnitOfMeasure | Unidades válidas/canónicas del producto. | Cardinalidad: **Unknown** hasta definir conversiones. | **Unresolved:** una futura conversión/asignación podría ser identificante por Product, Unit y contexto. No se inventa conversión. |
| StockOperation → InventoryMovement | Asientos que realizan la operación. | Operación draft `0..*`, confirmada `1..*`; cada movimiento → `1` operación. | Identificante conceptualmente: el asiento carece de significado fuera de la operación, aunque tenga UUID para trazabilidad. La transferencia agrupa efectos compensados. |
| Product → InventoryMovement | Producto afectado. | Product `1` → `0..*`; cada movimiento → `1` Product. | No identificante; producto/unidad compatibles. |
| Depot → InventoryMovement | Ubicación afectada. | Depot `1` → `0..*`; cada movimiento → `1` Depot. | No identificante; cierre no borra ledger. |
| StockOperation ↔ source Document/Event | Origen explicativo. | Operación requiere un origen semántico al confirmar; forma y máximo: Unknown según tipo. | **Unresolved** hasta tipificar origen y repetición. No usar un FK polimórfico físico sin diseño aprobado. |
| OperationalUnit → AgriculturalWork | Lugar de la labor. | UOP `1` → `0..*`; cada labor → exactamente `1` UOP. | No identificante. Campaign se deriva de UOP salvo snapshot justificado. |
| AgriculturalWork → WorkInputApplication | Insumos aplicados. | Labor `1` → `0..*`; cada aplicación → `1` labor. | Identificante para `WorkInputApplication` junto con Product y ocurrencia: la aplicación no existe sin la labor y el insumo. Conserva cantidad/unidad/estado. |
| WorkInputApplication ↔ InventoryMovement | Consumo de stock que realiza la aplicación. | Aplicación `0..*` movimientos; cada movimiento de consumo `0..1` aplicación. Reglas exactas Unknown. | No identificante: aplicación y asiento son hechos con identidades semánticas distintas; el vínculo es trazable y la anulación coordina reversión. |
| OperationalUnit → Harvest | Producción originada. | UOP `1` → `0..*`; cada cosecha → `1` UOP. | No identificante; preserva campaña mediante UOP. |
| Harvest → StockOperation | Ingreso de grano. | Harvest draft `0`, confirmada exactamente `1` operación lógica; operación → `0..1` Harvest. | No identificante: cosecha y operación de stock son hechos distintos. Vínculo idempotente, reversible e histórico. |
| OperationalAsset → MeterReading/AssetUsage/MaintenanceEvent | Historia del activo. | Activo `1` → `0..*` de cada evento; cada evento → `1` activo. | No identificante; eventos confirmados no se eliminan. |
| AssetUsage ↔ operational target | UOP, labor, rodeo o logística beneficiaria. | Máximos/obligatoriedad: **Unknown**. | **Unresolved:** si existe `UsageAllocation`, su dependencia identificante exige definir target, cantidad/porcentaje y ocurrencia. Decidir Etapa 6. |
| LivestockEvent ↔ Establishment/HerdGroup/LivestockCategory | Estado de origen/destino según tipo de evento. | Extremos opcionales por tipo; cantidad de efectos por evento: **Unknown**. | **Unresolved** hasta conocer granularidad y modelo de efectos. Reglas por tipo deben preservar conservación; decidir Etapa 7. |
| CPE ↔ Establishment/Campaign/OperationalUnit | Contexto operativo asignado a una CPE importada o preparada localmente. | El vínculo es opcional durante bandeja/vinculación; máximos por tipo y obligatoriedad final: **Unknown**. | **Unresolved:** un simple vínculo puede ser no identificante; una asignación con kilos/origen/vigencia sería identificante por participantes/ocurrencia. No altera datos legales ARCA. |
| CPE/Certificate ↔ StockOperation/InventoryMovement | Trazabilidad entre documento comercial/legal y flujo físico de grano. | Opcional en ambos extremos; máximos, unidad de enlace y obligatoriedad final: **Unknown**. | **Unresolved:** si distribuye cantidades, el hecho asociativo será identificante por participantes/ocurrencia e history-preserving. Resolver Etapa 8/9. |
| Contract → Product/Campaign/OperationalCounterparty | Términos básicos del compromiso comercial. | Contract draft `0..1` de cada uno; para confirmar, exactamente `1` Product, `1` Campaign y `1` Counterparty según Plan Etapa 9. Cada destino → `0..*` Contracts. | No identificantes respecto de los tres maestros: Contract conserva identidad semántica propia. Las referencias pertenecen a su versión; cambiar términos confirmados preserva historia. |
| Contract ↔ CPE | Kilos enviados/asignados sin orden de creación rígido. | `0..*` a `0..*`; ambos pueden existir sin vínculo. | Identificante para `ContractCpeAllocation`: la asignación no existe sin Contract y CPE; ocurrencia/versión completará su identidad natural. Es history-preserving y porta cantidad/estado/origen/razón según reglas aprobadas. |
| CPE ↔ Certificate | CPE comprendidas por certificados. | Ambos nodos pueden existir sin vínculo durante import/linking; obligatoriedad final y máximos: **Unknown**. | **Unresolved** hasta conocer atributos/repetición. Si lleva kilos/estado/origen, crear asociación identificante específica; no FK único. |
| Contract ↔ Certificate | Navegación/atribución comercial derivada o directa. | Ambos nodos pueden existir sin vínculo; máximos y necesidad de persistencia directa: **Unknown**. | **Unresolved:** primero decidir si es derivada o hecho propio. No duplicar un hecho derivable sin semántica; resolver Etapa 9. |
| Settlement ↔ Contract | Liquidación parcial/final asociada. | Contract `0..*` settlements; cantidad de contratos por Settlement: **Unknown**. | **Unresolved:** nodos con existencia independiente, pero una asignación con kilos/importes podría ser identificante. |
| Settlement ↔ CPE | Kilos/documentos considerados por liquidación. | Opcional durante import y linking; máximos y obligatoriedad final: **Unknown**. | **Unresolved:** posible asociación identificante con cantidad/estado; nunca exige cronología. |
| Settlement ↔ Certificate | Certificados considerados por liquidación. | Opcional durante import y linking; máximos y obligatoriedad final: **Unknown**. | **Unresolved:** posible asociación identificante con semántica propia; no borrar al corregir. |
| CPE ↔ Freight | Transporte operativo documentado. | Opcional en ambos extremos; máximos y obligatoriedad final: **Unknown**. | **Unresolved:** una asociación con tramo/cantidad puede ser identificante; decidir Etapa 9. |
| CostRecord ↔ operational target | Costo atribuible a múltiples objetos y viceversa. | `0..*` a `0..*`. | Identificante para `CostAllocation`: la asignación no existe sin CostRecord y target; método/vigencia/ocurrencia completarán su identidad natural. Conserva historia. |
| InternalRate → CostAllocation/result | Criterio interno aplicado. | `1` rate/version → `0..*` usos; uso puede ser `0..1` rate según clase. | No identificante: allocation/result conserva identidad propia. La versión aplicada debe quedar fijada; no revalorizar silenciosamente. |
| ReconciliationRun → ReconciliationDifference | Hallazgos de una corrida. | Run `1` → `0..*`; difference → `1` run. | Identificante conceptualmente: la diferencia observada no existe fuera de la corrida y el sujeto comparado. Los resultados no se borran al resolver. |
| ReconciliationDifference → Alert | Señal accionable deduplicada. | El vínculo puede ser opcional; máximos en ambos sentidos y agrupación entre corridas: **Unknown**. | **Unresolved:** depende de si Alert tiene identidad propia agregando corridas o es un vínculo débil. Resolver con regla de deduplicación Etapa 13. |
| User → Alert resolution | Responsable de cierre. | Alert abierta `0`; cerrada `1` responsable humano o proceso autorizado. | No identificante: Alert conserva identidad propia. Autoría no ownership; causa/momento quedan auditados. |

## 11. Estrategia de identidad y claves

Se distinguen ocho conceptos:

1. **Identidad interna/surrogate:** UUID estable, único y no nulo de Agro Ops. Es formalmente una clave candidata y normalmente la clave primaria elegida para referencias estructurales locales.
2. **Clave candidata:** conjunto mínimo de atributos que identifica un hecho según reglas probadas. El UUID es una; pueden existir además claves candidatas naturales, legales o de negocio, que no se inventan por conveniencia.
3. **Clave de negocio:** identificador con significado para el negocio, no necesariamente único ni inmutable.
4. **Código canónico estable:** código local controlado para integración/navegación. Sólo es clave candidata si se aprueba su scope y unicidad.
5. **Clave legal:** identidad definida por la autoridad legal, como la que finalmente se confirme para CPE o Settlement.
6. **Número legible/de documento:** etiqueta para personas; no se asume única, global ni inmutable.
7. **ID de implementación externo:** ID interno de ARCA, Finnegans o proveedor. Nunca estructura la identidad local.
8. **Mapping externo:** `ExternalReference` enlaza UUID local con sistema, tipo, namespace y external ID/version.

Supabase Auth se vincula mediante el subject estable del proveedor; email no identifica al `User`. ARCA define la identidad legal de CPE, mientras Agro Ops le asigna UUID local para relaciones. Finnegans conserva sus IDs internos, que sólo aparecen en mappings del adapter.

Una identidad legal o canónica con reglas y lifecycle propios pertenece a la entidad de dominio (por ejemplo, la clave legal aprobada de CPE). `ExternalReference` sirve para IDs técnicos, aliases y mapeos de sincronización; no reemplaza el concepto legal.

Una clave legal/canónica externa se conserva como dato autoritativo del sistema dueño y puede ser candidata sólo después de confirmar su definición y namespace. Sigue siendo distinta tanto del UUID local como del ID técnico usado por la implementación externa.

## 12. Modelo Organization, autenticación y autorización

V1 tiene exactamente un contexto operacional `Organization`, pero se conserva identidad interna para no confundir singleton de despliegue con clave universal. Todos los registros operativos pertenecen a la compañía, directa o transitivamente; no se particionan por creador.

Cada persona usa una cuenta individual. `ExternalAuthIdentity` traduce el subject estable de Supabase Auth a `User`. Supabase autentica y maneja sesión; el backend decide capacidades efectivas (ADR-005). Un usuario autenticado sin capacidad sigue no autorizado.

`Role` agrupa `Capability` mediante `RoleCapability`; `UserRole` asigna roles. La decisión semántica se formula en capacidades, no en nombres de roles. Cambios de membresía/concesión se auditan y no reinterpretan silenciosamente una decisión histórica.

Autoría (`created_by`, `changed_by` conceptuales) responde quién actuó. `AuditEvent` responde qué ocurrió, cuándo, por qué y con qué estado antes/después. Ninguno convierte al actor en dueño del dato.

## 13. Modelo territorial y geoespacial

- `Establishment` representa el campo/propiedad física completa y almacena su perímetro como `MultiPolygon` SRID 4326.
- `BasePlot` aporta una subdivisión interna estable y geometría territorial base `MultiPolygon` SRID 4326. Su geometría puede versionarse ante cambios físicos reales.
- `Campaign` pertenece a Organization y aporta contexto temporal/productivo; no posee Establishment directamente ni se inserta como eslabón físico ficticio entre BasePlot y UOP.
- `OperationalUnit` pertenece a una Campaign y un Establishment, usa geometría `MultiPolygon` SRID 4326 y se vincula M:N con uno o más `BasePlot` del mismo Establishment.
- `OperationalUnitUse` conserva actividad/cultivo/uso y vigencia.

Todas las geometrías territoriales usan SRID 4326 como almacenamiento/intercambio canónico. Las superficies derivadas deben aplicar una estrategia geográfica/proyectada correcta y reproducible, nunca grados como metros. Una geometría inválida no puede confirmarse como territorio operativo: debe rechazarse sin “arreglo” silencioso.

La geometría de una UOP debe estar cubierta por la unión de sus `BasePlot`; puede usar sólo una parte de un lote o agrupar varios. Dos UOP de la misma Campaign y Establishment pueden compartir borde, pero no área interior, contención ni geometría equivalente. No se aplica tolerancia: el snapping pertenece al flujo futuro de ingreso geográfico. Entre Campaign distintas el solapamiento es válido y esperado. La cobertura completa de los BasePlot no es obligatoria.

Invariante esencial: campaña N+1 puede dividir o unir el territorio de modo diferente sin modificar geometría, asignación ni explicación de campaña N. Una UOP puede cubrir múltiples BasePlot y un BasePlot puede participar en múltiples UOP mediante vínculos normalizados, siempre respetando organización, establecimiento, cobertura y no solapamiento interior dentro de la misma Campaign.

## 14. Grafo comercial de granos

`Contract`, `CPE`, `Certificate` y `Settlement` son nodos con existencia propia. Los vínculos representan asociaciones de negocio, no prerrequisitos de creación:

- CPE puede existir antes del Contract y Contract antes de CPE.
- El vínculo puede agregarse después y ser parcial.
- Contract–CPE es M:N y `ContractCpeAllocation` preserva sus atributos; no se reduce a un FK.
- Certificate puede referir CPE ya existentes; el import sequence no redefine identidad.
- Settlement llega más tarde desde Finnegans o puede quedar pendiente de links internos.
- “Parcial/final” es lifecycle/semántica de liquidación y asignación, no orden estructural obligatorio.
- Una liquidación final puede reutilizar CPE ya vinculadas sin exigir duplicación de relaciones.

Se distinguen:

- **Dependencia de existencia:** sólo existe cuando un concepto no tiene sentido sin otro; no se ha demostrado entre los cuatro nodos.
- **Asociación de negocio:** vínculos opcionales, potencialmente M:N y con atributos.
- **Workflow temporal:** orden en que personas registran o vinculan hechos; no se codifica como FK obligatorio.
- **Secuencia de importación:** orden de disponibilidad de ARCA/Finnegans; no cambia identidad ni cardinalidad.

Las reglas exactas de sobreasignación, unidad/cantidad por vínculo, relación Certificate–CPE y distribución parcial/final siguen pendientes. No se inventan.

## 15. Modelo de referencias externas

`ExternalReference` es adecuado cuando un registro local necesita mapear un ID técnico, tipo, versión o estado de sincronización de un sistema externo. Debe admitir namespaces explícitos y conservar cambios/anulaciones. No se infieren mapeos por descripciones libres.

Una identidad externa merece representación de dominio propia cuando tiene significado legal/canónico, atributos, invariantes, lifecycle o asociaciones propios. Por eso CPE no es sólo `ExternalReference`: conserva un UUID interno y la clave legal de ARCA una vez confirmada, mientras mappings técnicos permanecen separados.

### ARCA

- ARCA es autoridad legal de CPE/CTG.
- Agro Ops puede replicar, normalizar y relacionar CPE para operación.
- Agro Ops no redefine identidad ni modifica/anula legalmente CPE en V1.
- IDs de implementación ARCA no son PK ni identidad de dominio.
- El adapter ARCA aísla WSAA/WSCPE/SOAP/XML y sincroniza idempotentemente (ADR-009).

### Finnegans

- Finnegans es fuente contable/fiscal y de liquidación fiscal.
- Agro Ops no depende de BProc ni de estructuras/IDs internos de Finnegans.
- El adapter traduce por ports y mappings; `ExternalReference` registra equivalencias sin filtrar identidad externa al dominio.
- Sólo cruzan consecuencias y datos necesarios; no se reproducen contabilidad ni workflows ERP completos (ADR-010).

## 16. Política de tipos críticos

| Tipo conceptual | Regla |
|---|---|
| Money | Importe decimal exacto más moneda explícita. Precisión, escala y redondeo se fijan con reglas del circuito; no se presumen. |
| Kilograms, liters, hectares, hours, kilometers | Decimal exacto más unidad explícita. No `float`/`double`. |
| Percentages/rates | Decimal exacto, base (`0–1` o `0–100`) y regla de redondeo explícitas. |
| Other quantities/head count | Dimensión y unidad explícitas; cabezas enteras salvo que el dominio aprobado disponga otra magnitud. |
| Timestamp | Instante con zona/offset normalizado y política de presentación por zona. Eventos registran instante inequívoco. Política concreta de zona canónica: pendiente Etapa 2. |
| Date | Fecha civil sin fingir instante; campañas, vencimientos o documentos usan Date sólo si el negocio no requiere hora. |
| Status | Estado de dominio con transiciones autorizadas; no etiqueta UI ni texto externo sin traducir. |
| Geography | Geometría PostGIS apropiada y SRID explícito; no JSON/texto como autoridad geométrica (ADR-003). |
| Audit metadata | Actor interno o proceso, instante, causa/correlación, origen y before/after o referencias suficientes según sensibilidad. No define ownership. |

No se prescribe precisión/escala PostgreSQL arbitraria en esta etapa.

## 17. Catálogo de invariantes transversales

Clasificación: **C** conceptual, **DB** candidato a constraint de base futura, **APP** regla futura de dominio/aplicación. La clasificación no implementa el mecanismo.

| Invariante | Clase |
|---|---|
| Los UUID internos son estables e inmutables. | C, DB |
| IDs externos no reemplazan identidad interna Agro Ops. | C, DB, APP |
| Los datos operativos pertenecen a Organization, no al usuario creador. | C, DB, APP |
| Un subject estable de Supabase Auth mapea a un único User en su namespace; email no es identidad. | C, DB, APP |
| Roles agrupan capacidades y la autorización efectiva evalúa capacidades. | C, APP; DB para integridad de asignaciones |
| Autoría y audit no implican ownership. | C, APP |
| Una UOP pertenece a una campaña y nunca reescribe geografía de campañas previas. | C, DB, APP |
| Una UOP pertenece a un Establishment y su geometría está cubierta por la unión de sus BasePlot vinculados. | C, DB |
| Las UOP de una misma Campaign y Establishment no se solapan en su interior; compartir borde es válido. | C, DB |
| Cambiar BasePlot físicamente conserva la interpretación territorial histórica. | C, APP; DB según diseño temporal |
| Geometrías confirmadas son válidas y usan SRID aprobado. | C, DB, APP |
| El saldo de stock es suma exacta de movimientos confirmados; no se edita directamente. | C, APP; DB donde sea expresable |
| Transferencias internas conservan cantidad global. | C, DB, APP |
| Movimientos/eventos confirmados no se reescriben ni eliminan destructivamente. | C, DB, APP |
| Reversión/anulación referencia el hecho corregido y preserva ambos. | C, DB, APP |
| Reintentar una misma intención no duplica el efecto de negocio. | C, DB, APP |
| Cambio local, AuditEvent y OutboxEvent requerido se confirman atómicamente. | C, DB, APP |
| ARCA conserva identidad y estado legal autoritativo de CPE. | C, APP |
| Finnegans conserva verdad contable/fiscal; sus IDs internos no filtran identidad local. | C, DB, APP |
| Cantidades y dinero críticos no pierden precisión por coma flotante binaria. | C, DB, APP |
| Las unidades son explícitas y semánticamente compatibles. | C, DB, APP |
| Una relación M:N permitida no se colapsa en un único FK. | C, DB |
| Los atributos significativos de una relación se conservan en un concepto asociativo. | C, DB |
| Relaciones estructurales no codifican cronología accidental ni orden de importación. | C, DB, APP |
| Correcciones externas, legales y operativas conservan procedencia, versiones o compensaciones trazables. | C, DB, APP |
| Un documento/binario confirmado conserva hash, procedencia y versión; reemplazarlo no borra evidencia. | C, DB, APP |
| Cerrar una alerta conserva causa, responsable y audit. | C, DB, APP |

## 18. Mapa de primera implementación

| Etapa | Entidades cuya primera necesidad física aparece allí | Motivo |
|---|---|---|
| Etapa 2 — transversal core | Organization, User, ExternalAuthIdentity, Role, Capability, UserRole, RoleCapability, AuditEvent, IdempotencyRecord, Job, OutboxEvent, Document, StoredObject, DocumentLink, ExternalReference | Capacidades reutilizables exigidas por Plan Etapa 2. |
| Etapa 3 — territory | Establishment, BasePlot, Campaign, OperationalUnit, ActivityUse, OperationalUnitUse | Primera vertical y cartografía histórica. |
| Etapa 4 — inventory | Product, UnitOfMeasure, Depot, StockOperation, InventoryMovement | Ledger de insumos; luego también grano. |
| Etapa 5 — agriculture | FieldWorkType, AgriculturalWork, WorkInputApplication, Harvest | Labor → consumo → cosecha. |
| Etapa 6 — machinery/vehicles | OperationalAsset, MeterReading, AssetUsage, MaintenanceEvent | Uso, lecturas y mantenimiento operativo. |
| Etapa 7 — livestock | LivestockCategory, HerdGroup, LivestockEvent | Existencias/eventos según granularidad aprobada. |
| Etapa 8 — grain | CPE, Certificate, Settlement | Representación operativa/documental y preparación del grafo antes de adapters, como exige Plan Etapa 8. Los datos de contraparte presentes en documentos se conservan como snapshot legal/externo sin exigir aún un maestro local. |
| Etapa 9 — grain commercialization | OperationalCounterparty, Contract, ContractCpeAllocation, Freight | El primer uso probado de contraparte local es el Contract; se agregan contratación, asignaciones parciales y logística. Asociaciones adicionales sólo si se aprueba su semántica. |
| Etapa 10 — ARCA | Ninguna entidad de dominio nueva aprobada; se materializan sólo estados/metadata de integración que el diseño ARCA demuestre necesitar | CPE local ya existe; el adapter no justifica duplicarla. |
| Etapa 11 — Finnegans | CostRecord | Primera recepción local de costos/documentos relevantes. Metadata técnica adicional debe derivarse de Job/Outbox/ExternalReference. |
| Etapa 12 — costs/results | CostAllocation, InternalRate, ProductiveResult | Fuentes ya confiables habilitan asignación y resultados. |
| Etapa 13 — reports/reconciliation/alerts | ReconciliationRun, ReconciliationDifference, Alert | Sólo estos conceptos requieren lifecycle persistente; reportes/proyecciones no son entidades por defecto. |

Ninguna entidad se materializa en Etapa 1.5. Esta tabla no concede autorización automática para crear tablas en etapas futuras; aplica siempre el gate de §6.

## 19. Matriz de revisión de normalización

| Relación conceptual candidata | Claves candidatas | FD importantes / determinantes | Forma máxima hoy justificable | BCNF | 4NF | 5NF | Riesgo de anomalía / decisión siguiente |
|---|---|---|---|---|---|---|---|
| Organization | UUID; clave natural/de negocio adicional Unknown | UUID → datos propios; otros determinantes Unknown | Unknown / not yet assessable from current business rules; política: ≥3NF | Target BCNF; compliance not yet assessable from current business rules | Ninguna MVD demostrada | Ninguna JD demostrada | No usar singleton como key. |
| User | UUID; clave natural adicional Unknown; email no candidata establecida | UUID → perfil/estado; otros determinantes Unknown | Unknown / not yet assessable; política: ≥3NF | Target BCNF; compliance not yet assessable | Ninguna MVD demostrada; Auth/roles ya se separan conceptualmente | Ninguna JD demostrada | No mezclar identidad Auth ni roles. |
| ExternalAuthIdentity | UUID; forma natural `(provider, subject)` pendiente de namespace | UUID → mapping; clave externa → User sólo al cerrar namespace | Unknown / not yet assessable; política: ≥3NF | Target BCNF; compliance not yet assessable | Ninguna MVD demostrada | Ninguna JD demostrada | Definir provider/tenant scope Etapa 2. |
| Role / Capability | UUID por entidad; códigos naturales adicionales Unknown | UUID → definición; otros determinantes Unknown | Unknown / not yet assessable; política: ≥3NF | Target BCNF; compliance not yet assessable | Ninguna MVD demostrada; M:N no implica violación 4NF | Ninguna JD demostrada | Aprobar códigos/alcance. |
| UserRole | UUID; clave natural adicional Unknown; `(User, Role)` condicionada por vigencia | UUID → membresía; FD del par Unknown | Unknown / not yet assessable; política: ≥3NF | Target BCNF; compliance not yet assessable | Ninguna MVD demostrada dentro de la asociación | Ninguna JD demostrada | Definir revocación/vigencia e identidad natural. |
| RoleCapability | UUID; clave natural adicional Unknown; `(Role, Capability)` condicionada por vigencia | UUID → concesión; FD del par Unknown | Unknown / not yet assessable; política: ≥3NF | Target BCNF; compliance not yet assessable | Ninguna MVD demostrada dentro de la asociación | Ninguna JD demostrada | Definir versionado e identidad natural. |
| AuditEvent | UUID; natural adicional Unknown | UUID → hecho; otros determinantes Unknown | Unknown / not yet assessable; política: ≥3NF | Target BCNF; compliance not yet assessable | Ninguna MVD demostrada | Ninguna JD demostrada | Evitar payload opaco como única evidencia consultable. |
| IdempotencyRecord | UUID; forma natural `(scope, key)` pendiente | UUID → resultado; FD scope/key pendiente | Unknown / not yet assessable; política: ≥3NF | Target BCNF; compliance not yet assessable | Ninguna MVD demostrada | Ninguna JD demostrada | Cerrar scope/retención antes de Etapa 2. |
| Job | UUID; natural adicional Unknown | UUID → definición/estado; determinantes de intentos Unknown | Unknown / not yet assessable; política: ≥3NF | Target BCNF; compliance not yet assessable | Ninguna MVD demostrada; separar historiales si aparecen colecciones independientes | Ninguna JD demostrada | No sobrescribir historia con sólo `last_error`. |
| OutboxEvent | UUID; natural adicional Unknown | UUID → evento/aggregate/payload; otros determinantes Unknown | Unknown / not yet assessable; política: ≥3NF | Target BCNF; compliance not yet assessable | Ninguna MVD demostrada; analizar intentos sólo si coexisten colecciones independientes | Ninguna JD demostrada | Preservar observabilidad sin arrays opacos. |
| Document / StoredObject / DocumentLink | UUID por entidad; naturales adicionales Unknown | UUID → hechos propios; otros determinantes Unknown | Unknown / not yet assessable; política: ≥3NF | Target BCNF; compliance not yet assessable | Afecta candidate keys / FD; 4NF debe analizarse sólo si una futura relación candidata combina dependencias multivaluadas genuinas e independientes de objetos y links | Ninguna JD demostrada; revisar ternaria sólo ante regla real | Definir cardinalidad/versiones Etapa 2. |
| ExternalReference | UUID; clave natural externa con scope Unknown | UUID → mapping; key externa → local sólo tras definir namespace | Unknown / not yet assessable; política: ≥3NF | Target BCNF; compliance not yet assessable | Ninguna MVD demostrada | Ninguna JD demostrada | Resolver unicidad por ambiente/conexión/tipo. |
| Establishment | UUID; código natural adicional Unknown | UUID → identidad/contexto; otros determinantes Unknown | Unknown / not yet assessable; política: ≥3NF | Target BCNF; compliance not yet assessable | Ninguna MVD demostrada | Ninguna JD demostrada | Scope de código pendiente. |
| BasePlot identity / geometry history | UUID; código natural adicional Unknown; key de versión Unknown | UUID → identidad/establishment; dependencias temporales Unknown | Unknown / not yet assessable; política: ≥3NF | Target BCNF; compliance not yet assessable | Ninguna MVD demostrada; versionado temporal no implica por sí solo 4NF | Ninguna JD demostrada | Diseñar vigencias sin sobrescritura Etapa 3. |
| Campaign | UUID; `(Organization, code)` | UUID → Organization/período/estado; `(Organization, code)` → Campaign | BCNF para atributos materializados | Sí | Ninguna MVD demostrada | Ninguna JD no trivial demostrada | Alcance organizacional cerrado; establecimientos se derivan de UOP. |
| OperationalUnit | UUID; `(Campaign, Establishment, code)` | UUID → Campaign, Establishment, geometría/estado; clave de scope → UOP | BCNF para atributos materializados; participación BasePlot separada | Sí | Participaciones BasePlot están en asociación M:N independiente | Ninguna JD no trivial demostrada | Identidad aislada por Campaign; cobertura y solapamiento se validan en DB. |
| OperationalUnitBasePlot | UUID; `(OperationalUnit, BasePlot)` | UUID → vínculo; par → vínculo y contexto coherente | BCNF | Sí | Una fila representa un solo vínculo; no mezcla colecciones independientes | Ninguna JD no trivial demostrada | Asociación M:N history-preserving sin arrays ni geometría copiada. |
| OperationalUnitUse | UUID; natural adicional Unknown | UUID → UOP/use/vigencia; otros determinantes Unknown | Unknown / not yet assessable; política: ≥3NF | Target BCNF; compliance not yet assessable | Ninguna MVD demostrada dentro de la asociación; M:N no basta | Ninguna JD demostrada | Definir multiplicidad/vigencia. |
| Product / UnitOfMeasure | UUID por entidad; códigos/conversion keys adicionales Unknown | UUID → definición; dependencias de conversión Unknown | Unknown / not yet assessable; política: ≥3NF | Target BCNF; compliance not yet assessable | Ninguna MVD demostrada; varias unidades no bastan para inferirla | Ninguna JD demostrada | Definir unidad canónica/conversiones Etapa 4. |
| Depot | UUID; código natural adicional Unknown | UUID → definición; otros determinantes Unknown | Unknown / not yet assessable; política: ≥3NF | Target BCNF; compliance not yet assessable | Ninguna MVD demostrada | Ninguna JD demostrada | Definir scope de código. |
| StockOperation | UUID; forma natural idempotente pendiente de scope | UUID → tipo/origen/estado; otros determinantes Unknown | Unknown / not yet assessable; política: ≥3NF | Target BCNF; compliance not yet assessable | Ninguna MVD demostrada; fuentes múltiples no bastan por sí solas | Ninguna JD demostrada | Formalizar tipos y origen. |
| InventoryMovement | UUID; natural adicional Unknown | UUID → operation/product/depot/qty/unit/status/time; otros determinantes Unknown | Unknown / not yet assessable; política: ≥3NF | Target BCNF; compliance not yet assessable | Ninguna MVD demostrada | Ninguna JD demostrada | No persistir saldo canónico; validar unidades. |
| AgriculturalWork | UUID; natural adicional Unknown | UUID → UOP/type/date/area/status; UOP → Campaign | Unknown / not yet assessable; política: ≥3NF | Target BCNF; compliance not yet assessable; evitar Campaign redundante salvo snapshot semántico | Afecta candidate keys / FD; 4NF debe analizarse sólo si una futura relación candidata combina dependencias multivaluadas genuinas e independientes de aplicaciones y asignaciones | Ninguna JD demostrada | Definir snapshots y asignaciones Etapa 5. |
| WorkInputApplication | UUID; natural adicional Unknown | UUID → work/product/qty/unit/state; otros determinantes Unknown | Unknown / not yet assessable; política: ≥3NF | Target BCNF; compliance not yet assessable | Ninguna MVD demostrada; varios movimientos no bastan por sí solos | Ninguna JD demostrada | Definir repetición y trazabilidad. |
| Harvest | UUID; forma natural idempotente pendiente | UUID → UOP/product/qty/date/status; otros determinantes Unknown | Unknown / not yet assessable; política: ≥3NF | Target BCNF; compliance not yet assessable | Ninguna MVD demostrada | Ninguna JD demostrada | Definir key idempotente Etapa 5. |
| Asset / readings / usage / maintenance | UUID por hecho; naturales adicionales Unknown | UUID → atributos propios; otros determinantes Unknown | Unknown / not yet assessable; política: ≥3NF | Target BCNF; compliance not yet assessable por relación candidata | Ninguna MVD demostrada; afecta candidate keys / FD y 4NF debe analizarse sólo si una futura relación candidata combina dependencias multivaluadas genuinas e independientes | Ninguna JD demostrada | Definir UsageAllocation y correcciones Etapa 6. |
| LivestockEvent | UUID; forma natural idempotente pendiente | UUID → tipo/fecha/magnitudes/efectos; otros determinantes Unknown | Unknown / not yet assessable; política: ≥3NF | Target BCNF; compliance not yet assessable | Posible MVD sólo si surgen conjuntos independientes de sujetos/efectos; no demostrada | Ninguna JD demostrada | Resolver granularidad Etapa 7. |
| CPE | UUID; clave legal natural adicional Unknown | UUID → stream local; legal key → stream sólo cuando se apruebe | Unknown / not yet assessable; política: ≥3NF | Target BCNF; compliance not yet assessable | Afecta candidate keys / FD; 4NF debe analizarse sólo si una futura relación candidata combina dependencias multivaluadas genuinas e independientes de links | Ninguna JD demostrada; revisar grafo sólo ante regla real | Confirmar clave ARCA antes de Etapa 8. |
| Certificate | UUID; legal/business key adicional Unknown | UUID → representación local; otros determinantes Unknown | Unknown / not yet assessable; política: ≥3NF | Target BCNF; compliance not yet assessable | Afecta candidate keys / FD; 4NF debe analizarse sólo si una futura relación candidata combina dependencias multivaluadas genuinas e independientes de links a CPE y stock | Ninguna JD demostrada | Resolver owner, keys y links antes de Etapa 8/9. |
| Settlement | UUID; fiscal key natural adicional Unknown | UUID → stream local; fiscal key pendiente | Unknown / not yet assessable; política: ≥3NF | Target BCNF; compliance not yet assessable | Afecta candidate keys / FD; 4NF debe analizarse sólo si una futura relación candidata combina dependencias multivaluadas genuinas e independientes de links a Contract, CPE y Certificate | Ninguna JD demostrada; revisar grafo sólo ante regla real | Resolver identidad Etapa 8 y parciales Etapa 9. |
| Contract | UUID; number/code natural adicional Unknown | UUID → terms/version; otros determinantes Unknown | Unknown / not yet assessable; política: ≥3NF | Target BCNF; compliance not yet assessable | Afecta candidate keys / FD; 4NF debe analizarse sólo si una futura relación candidata combina dependencias multivaluadas genuinas e independientes de allocations, certificates y settlements | Ninguna JD demostrada; revisar sólo ante dependencia real | Scope de número y lifecycle Etapa 9. |
| ContractCpeAllocation | UUID; natural adicional Unknown | UUID → Contract/CPE/qty/state/origin; otros determinantes Unknown | Unknown / not yet assessable; política: ≥3NF | Target BCNF; compliance not yet assessable | Ninguna MVD demostrada dentro de la asociación | Ninguna JD demostrada; no presumir ternaria | Definir reglas de cantidad y uniqueness Etapa 9. |
| Freight | UUID; external number no es key establecida | UUID → servicio/estado; otros determinantes Unknown | Unknown / not yet assessable; política: ≥3NF | Target BCNF; compliance not yet assessable | Posible MVD sólo si tramos y CPE son conjuntos independientes combinados; no demostrada | Ninguna JD demostrada | Definir cardinalidad Etapa 9. |
| CostRecord | UUID; fiscal key natural adicional Unknown | UUID → type/amount/currency/date/provenance; otros determinantes Unknown | Unknown / not yet assessable; política: ≥3NF | Target BCNF; compliance not yet assessable; separar subtipos si aparecen determinantes distintos | Posible MVD si referencias y allocations independientes se combinaran | Ninguna JD demostrada | Validar contrato Finnegans Etapa 11. |
| CostAllocation | UUID; natural adicional Unknown | UUID → cost/target/value/method/vigencia; otros determinantes Unknown | Unknown / not yet assessable; política: ≥3NF | Target BCNF; compliance not yet assessable | Ninguna MVD demostrada dentro de la asociación; múltiples targets no bastan | Ninguna JD demostrada; revisar cost–target–method sólo ante regla real | Reglas de asignación Etapa 12. |
| InternalRate / ProductiveResult | UUID por entidad; scope keys adicionales Unknown | UUID → vigencia/value o resultado/inputs; otros determinantes Unknown | Unknown / not yet assessable; política: ≥3NF | Target BCNF; compliance not yet assessable | Ninguna MVD demostrada | Ninguna JD demostrada | Definir políticas reproducibles Etapa 12. |
| ReconciliationRun / Difference / Alert | UUID por hecho; dedup keys adicionales Unknown | UUID → atributos propios; otros determinantes Unknown | Unknown / not yet assessable; política: ≥3NF | Target BCNF; compliance not yet assessable | Ninguna MVD demostrada dentro de cada relación | Ninguna JD demostrada | Definir deduplicación y lifecycle Etapa 13. |

No hay hoy una dependencia de join no trivial demostrada que obligue 5NF. El grafo comercial y las posibles ternarias territoriales/costos deben revisarse en su etapa, pero no descomponerse preventivamente.

## 20. Preguntas y decisiones pendientes

| Pregunta | Por qué importa / entidades afectadas | Decisión dependiente | Fecha límite |
|---|---|---|---|
| ¿Cuál es el scope y unicidad de códigos legibles/canónicos de Organization, Role, Capability y demás maestros? | Evita falsas keys globales y mappings ambiguos. | Claves naturales candidatas adicionales, constraints y versionado de códigos. | Cada relación: no después de su etapa; core en Etapa 2. |
| ¿Cuál es el namespace exacto de Supabase Auth subject? | Asegura mapping 1:1 estable sin email. | Clave natural candidata adicional de ExternalAuthIdentity. | Etapa 2. |
| ¿UserRole y RoleCapability admiten múltiples vigencias para el mismo par y qué atributos completan su identidad natural? | El UUID ya es clave candidata surrogate, pero el carácter identificante no determina por sí solo la clave natural ni las FD temporales. | Claves naturales adicionales, vigencia, revocación y normalización de las asociaciones. | Etapa 2. |
| ¿Cuál es el scope de unicidad de ExternalReference (sistema, conexión/empresa, ambiente, tipo)? | Evita colisiones y duplicación idempotente. | Clave natural candidata adicional y FD external key → local entity. | Etapa 2. |
| ¿Qué política canónica de timezone usa timestamps? | Orden y audit inequívocos entre sistemas. | Normalización de instantes y validación de fechas. | Etapa 2. |
| ¿Document puede tener múltiples StoredObject/versiones y cuál identifica la versión vigente? | Afecta candidate keys / FD e historia documental; 4NF debe analizarse sólo si una futura relación candidata combina dependencias multivaluadas genuinas e independientes. | Descomposición Document–StoredObject–DocumentLink. | Etapa 2. |
| ¿Cómo se versiona un cambio físico real de BasePlot y cuál es la fuente inicial de geometría? | Una actualización simple podría reinterpretar campañas pasadas. | Identidad vs versión geométrica y vigencia. | Etapa 3. |
| ¿Una UOP admite varios usos simultáneos o secuenciales y con qué vigencia? | Afecta candidate keys / FD de OperationalUnitUse; 4NF debe analizarse sólo si una futura relación candidata combina dependencias multivaluadas genuinas e independientes. | Cardinalidad y constraints de uso. | Etapa 3. |
| ¿Qué unidades/conversiones acepta cada Product y qué redondeos rigen? | Evita sumar cantidades incompatibles. | Product–UnitOfMeasure, exactitud y claves naturales candidatas adicionales de conversiones. | Etapa 4. |
| ¿Se permite stock negativo por producto/depósito y qué excepciones autorizadas existen? | Determina concurrencia y validez del ledger. | Invariantes de confirmación. | Etapa 4. |
| ¿Qué formas de origen puede tener StockOperation y cómo se identifica una operación fuente sin FK polimórfico ambiguo? | Cada movimiento debe ser explicable y normalizable. | Relación con Document/evento, FDs e integridad referencial. | Etapa 4. |
| ¿Cuántos movimientos pueden realizar una WorkInputApplication y cómo se revierte la aplicación completa? | Evita consumos huérfanos o parcialmente revertidos. | Cardinalidad WorkInputApplication–InventoryMovement e invariant transaccional. | Etapa 5. |
| ¿Un AssetUsage puede distribuirse entre varios targets y con qué magnitud? | Puede exigir `UsageAllocation`; sólo habría cuestión 4NF si se demostraran dependencias multivaluadas independientes dentro de una relación candidata. | Cardinalidad, asociación y conservación de horas/km/ha. | Etapa 6. |
| ¿La ganadería V1 opera por individuo, categoría, rodeo o combinación? | Define identidad del sujeto, event effects, keys y FDs. | Diseño físico de LivestockEvent y saldos. | Etapa 7. |
| ¿Cuál es la clave legal exacta de CPE/CTG y su namespace? | Import repetido debe identificar un único stream legal sin usar ID ARCA interno. | Clave legal candidata adicional de CPE e idempotencia. | Antes de materializar CPE en Etapa 8; validar de nuevo en POC Etapa 10. |
| ¿Quién es sistema de registro del Certificate y cuál es su clave legal/de negocio? | Cambia mutabilidad, sync, keys y vínculos con stock/CPE. | Diseño de Certificate. | Etapa 8. |
| ¿Cuál es la clave fiscal de Settlement independiente del ID interno Finnegans? | Permite réplica idempotente antes del adapter definitivo. | Clave fiscal candidata adicional y lifecycle de Settlement. | Etapa 8; validar adapter Etapa 11. |
| ¿Cuál es la identidad operativa mínima de OperationalCounterparty y su relación con maestro fiscal Finnegans? | Evita duplicar o depender estructuralmente del ERP; los snapshots de contraparte en CPE no prueban por sí solos un maestro local. | Keys y mappings de contraparte. | Etapa 9. |
| ¿Qué cardinalidad y atributos tienen CPE–territorio y CPE/Certificate–stock? | Los vínculos pueden repartir kilos entre UOP, campañas o movimientos; un FK único perdería semántica. | Asociaciones, claves candidatas adicionales, FD e invariantes; 4NF debe analizarse sólo si una futura relación candidata combina dependencias multivaluadas genuinas e independientes. | Etapa 8 para el modelo base; completar reglas comerciales en Etapa 9. |
| ¿Qué atributos, reglas de sobreasignación y uniqueness tiene ContractCpeAllocation? | La cantidad parcial y corrección son hechos del vínculo. | Clave natural candidata adicional, FDs, reversión y constraints. | Etapa 9. |
| ¿Qué cardinalidades/atributos exactos tienen Certificate–CPE y Settlement–Contract/CPE/Certificate? | Decide asociaciones y balances; afecta candidate keys / FD y 4NF debe analizarse sólo si una futura relación candidata combina dependencias multivaluadas genuinas e independientes; 5NF requiere una join dependency no trivial, ninguna demostrada todavía. | Grafo comercial y balances. | Etapa 9. |
| ¿Cuál es lifecycle exacto de Settlement parcial/final y de sus correcciones/anulaciones? | Evita codificar cronología y perder historia. | Estados, FDs y balance liquidado. | Etapa 9; contrato externo validado Etapa 11. |
| ¿Qué scope y condición de clave natural candidata adicional tiene el número/código de Contract? | Evita unicidad inventada. | Constraints e idempotencia de contrato. | Etapa 9. |
| ¿Qué cardinalidad y atributos tiene CPE–Freight (tramos, cantidad, costo)? | Puede ser M:N y portar semántica logística. | Necesidad de asociación, keys y conservación de kilos/costos. | Etapa 9. |
| ¿Qué mecanismo estándar y claves externas ofrece Finnegans, sin BProc? | Determina mappings e idempotencia de costos/documentos. | ExternalReference y CostRecord. | Etapa 11. |
| ¿Qué reglas exactas rigen tarifas internas, depreciación y asignación? | Determina CostAllocation/InternalRate y resultados reproducibles. | Keys, FDs, redondeos e historia. | Etapa 12. |
| ¿Cómo se deduplican diferencias/alertas activas y qué tolerancias aplican? | Reejecuciones no deben duplicar alertas ni ocultar diferencias. | Claves naturales candidatas adicionales y lifecycle de conciliación. | Etapa 13. |

## 21. Contrato para el futuro ERD gráfico

La siguiente slice de Etapa 1.5 derivará directamente de este documento aprobado:

`docs/data-model/agro-ops-erd.mmd`

Ese Mermaid deberá:

- incluir sólo entidades conceptuales aprobadas;
- reflejar cardinalidad y opcionalidad aprobadas;
- representar entidades asociativas cuando el vínculo tenga semántica propia;
- marcar o excluir relaciones todavía no resueltas sin decidirlas silenciosamente;
- no inventar entidades, keys ni orden cronológico;
- no sustituir este catálogo de invariantes, dependencias, lifecycle y preguntas.

Opcionalmente podrá generarse `docs/data-model/agro-ops-erd.svg` para lectura humana. El `.mmd` será la fuente gráfica editable/versionada; este documento seguirá siendo la fuente autoritativa de semántica. Ninguno de los dos archivos gráficos forma parte de Etapa 1.5A.
