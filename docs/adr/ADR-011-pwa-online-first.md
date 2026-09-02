# ADR-011: PWA online-first

## Estado

Aceptado

## Contexto

Agro Ops será utilizado desde:

- desktop;
- tablet;
- celular.

Parte de la operación ocurrirá en establecimientos rurales, donde la conectividad puede ser variable.

Sin embargo, implementar un sistema offline-first completo desde V1 introduciría complejidad considerable:

- almacenamiento local persistente;
- sincronización diferida;
- resolución de conflictos;
- versionado de registros;
- merge de cambios;
- colas locales;
- reintentos;
- detección de conectividad;
- consistencia entre múltiples dispositivos.

El sistema todavía estará evolucionando funcionalmente durante las primeras etapas.

## Decisión

Agro Ops se implementará como una Progressive Web App online-first.

La aplicación deberá:

- funcionar correctamente en navegadores modernos;
- adaptarse a desktop, tablet y celular;
- poder instalarse como PWA cuando el navegador lo permita;
- tener navegación y experiencia adecuadas para uso operativo;
- asumir conectividad para operaciones críticas de escritura en V1.

## Online-first

En V1, las operaciones críticas se confirmarán contra el backend.

Conceptualmente:

    usuario
      |
      v
    frontend
      |
      | conexión
      v
    backend
      |
      v
    PostgreSQL

Una operación no se considerará confirmada hasta recibir respuesta válida del backend.

## Sin conectividad

Cuando no exista conexión, la aplicación deberá:

- informar claramente el estado;
- evitar indicar que una operación fue guardada si no lo fue;
- preservar, cuando sea razonable, información visual ya cargada;
- permitir reintentar cuando vuelva la conectividad.

No se implementará en V1 una cola local general de operaciones críticas pendientes de sincronización.

## PWA

La aplicación podrá incluir capacidades como:

- instalación en pantalla de inicio;
- manifest;
- iconos;
- shell básico de aplicación;
- service worker cuando corresponda;
- caching controlado de recursos estáticos.

El uso de service workers no implica que la aplicación sea offline-first.

## Datos críticos

No se permitirá confirmar localmente sin backend operaciones como:

- movimientos de stock;
- labores;
- cosechas;
- contratos;
- asignación de CPE;
- movimientos ganaderos;
- ajustes;
- anulaciones;
- cambios con impacto económico.

Estas operaciones requieren validación autoritativa del backend.

## Validación

El frontend podrá realizar validaciones de experiencia de usuario.

Ejemplos:

- campos obligatorios;
- formato de fechas;
- números inválidos;
- advertencias visuales.

Sin embargo, las reglas críticas se validarán nuevamente en Rust.

El frontend nunca será la autoridad final.

## Diseño responsive

Las pantallas deberán diseñarse considerando distintos tamaños.

### Desktop

Prioridad para:

- dashboards;
- tablas;
- análisis;
- administración;
- conciliación.

### Tablet

Prioridad para:

- mapas;
- registros operativos;
- consulta en campo.

### Celular

Prioridad para:

- consulta rápida;
- carga de eventos simples;
- navegación contextual;
- operación esencial.

No todas las pantallas deberán tener exactamente la misma densidad de información en todos los dispositivos.

## Mapas

MapLibre deberá funcionar en dispositivos táctiles.

La interacción con lotes y unidades operativas deberá contemplar:

- click;
- tap;
- zoom;
- pan;
- selección de geometrías.

## Evolución futura

Un modo offline más avanzado podrá incorporarse después de estabilizar:

- modelo de dominio;
- workflows;
- conflictos posibles;
- permisos;
- reglas de sincronización;
- auditoría.

Una futura implementación offline-first deberá definir explícitamente:

- qué entidades pueden editarse offline;
- cómo se versionan;
- cómo se detectan conflictos;
- cómo se resuelven;
- cómo se auditan;
- qué operaciones nunca pueden confirmarse offline.

## Consecuencias

### Ventajas

- Menor complejidad inicial.
- Menor riesgo de inconsistencias.
- Desarrollo más rápido de las verticales operativas.
- Validaciones críticas permanecen centralizadas.
- Compatible con uso desde celular y tablet.
- Permite evolucionar hacia offline más adelante.

### Costos

- Algunas funciones no estarán disponibles sin conectividad.
- La operación en zonas sin señal dependerá de cobertura o reintento posterior.
- Puede ser necesario diseñar mecanismos offline específicos en futuras versiones.

## Alternativas consideradas

### Offline-first completo desde V1

No se selecciona porque agrega complejidad significativa antes de estabilizar el dominio.

### Aplicación móvil nativa

No se considera necesaria en V1.

La PWA cubre inicialmente los dispositivos requeridos con una única codebase frontend.

## Regla

Agro Ops V1 será PWA online-first.

No implementar sincronización offline general, resolución de conflictos distribuida o confirmación local de operaciones críticas sin una nueva decisión arquitectónica explícita.
