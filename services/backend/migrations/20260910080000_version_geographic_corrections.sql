INSERT INTO permisos (codigo, nombre, descripcion)
VALUES (
    'territorio:gestionar',
    'Gestionar territorio confirmado',
    'Permite corregir evidencia geográfica confirmada y reasignar sus contribuciones.'
)
ON CONFLICT (codigo) DO NOTHING;

ALTER TABLE fuentes_geograficas
    ADD CONSTRAINT uq_fuentes_geograficas_id_organizacion UNIQUE (id, organizacion_id),
    ADD COLUMN reemplaza_fuente_geografica_id UUID,
    ADD COLUMN motivo_correccion TEXT,
    ADD CONSTRAINT fk_fuentes_geograficas_reemplaza
        FOREIGN KEY (reemplaza_fuente_geografica_id, organizacion_id)
        REFERENCES fuentes_geograficas (id, organizacion_id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    ADD CONSTRAINT uq_fuentes_geograficas_reemplaza
        UNIQUE (reemplaza_fuente_geografica_id),
    ADD CONSTRAINT ck_fuentes_geograficas_motivo_correccion
        CHECK (
            (reemplaza_fuente_geografica_id IS NULL AND motivo_correccion IS NULL)
            OR
            (
                reemplaza_fuente_geografica_id IS NOT NULL
                AND motivo_correccion = btrim(motivo_correccion)
                AND char_length(motivo_correccion) BETWEEN 1 AND 1000
            )
        ),
    ADD CONSTRAINT ck_fuentes_geograficas_reemplazo_no_reflexivo
        CHECK (reemplaza_fuente_geografica_id IS NULL OR reemplaza_fuente_geografica_id <> id);

ALTER TABLE fuentes_geograficas
    DROP CONSTRAINT ck_fuentes_geograficas_version_parser,
    ADD CONSTRAINT ck_fuentes_geograficas_version_parser
        CHECK (
            (tipo_origen = 'senasa_renspa' AND version_parser IN (
                'senasa_pasted_polygon_v1', 'territory_correction_geojson_v1'
            ))
            OR
            (tipo_origen IN ('manual', 'importada') AND version_parser IN (
                'geojson_polygon_multipolygon_v1', 'territory_correction_geojson_v1'
            ))
        );

ALTER TABLE fuentes_geograficas_contribuciones_canonicas
    DROP CONSTRAINT uq_fuentes_geograficas_contribuciones_fuente,
    DROP CONSTRAINT fk_fuentes_geograficas_contribuciones_fuente,
    ADD COLUMN vigente_hasta TIMESTAMPTZ,
    ADD COLUMN cerrada_por UUID,
    ADD COLUMN reemplazada_por_contribucion_id UUID,
    ADD CONSTRAINT uq_fuentes_geograficas_contribuciones_id_organizacion
        UNIQUE (id, organizacion_id),
    ADD CONSTRAINT fk_fuentes_geograficas_contribuciones_fuente
        FOREIGN KEY (fuente_geografica_id, organizacion_id)
        REFERENCES fuentes_geograficas (id, organizacion_id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    ADD CONSTRAINT fk_fuentes_geograficas_contribuciones_cerrada_por
        FOREIGN KEY (cerrada_por)
        REFERENCES usuarios (id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    ADD CONSTRAINT fk_fuentes_geograficas_contribuciones_reemplazo
        FOREIGN KEY (reemplazada_por_contribucion_id, organizacion_id)
        REFERENCES fuentes_geograficas_contribuciones_canonicas (id, organizacion_id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT
        DEFERRABLE INITIALLY DEFERRED,
    ADD CONSTRAINT ck_fuentes_geograficas_contribuciones_vigencia
        CHECK (
            (vigente_hasta IS NULL AND cerrada_por IS NULL AND reemplazada_por_contribucion_id IS NULL)
            OR
            (
                vigente_hasta IS NOT NULL
                AND cerrada_por IS NOT NULL
                AND vigente_hasta >= confirmado_en
            )
        ),
    ADD CONSTRAINT ck_fuentes_geograficas_contribuciones_reemplazo_no_reflexivo
        CHECK (reemplazada_por_contribucion_id IS NULL OR reemplazada_por_contribucion_id <> id);

CREATE UNIQUE INDEX uq_fuentes_geograficas_contribuciones_fuente_activa
    ON fuentes_geograficas_contribuciones_canonicas (fuente_geografica_id)
    WHERE vigente_hasta IS NULL;

CREATE INDEX idx_fuentes_geograficas_contribuciones_activas_establecimiento
    ON fuentes_geograficas_contribuciones_canonicas
    (organizacion_id, establecimiento_id, fuente_geografica_id)
    WHERE vigente_hasta IS NULL;

CREATE FUNCTION proteger_historia_contribuciones_geograficas()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION USING ERRCODE = 'P0001',
            MESSAGE = 'agro_ops_contribucion_geografica_eliminacion_prohibida';
    END IF;

    IF OLD.vigente_hasta IS NOT NULL
        OR NEW.id IS DISTINCT FROM OLD.id
        OR NEW.organizacion_id IS DISTINCT FROM OLD.organizacion_id
        OR NEW.establecimiento_id IS DISTINCT FROM OLD.establecimiento_id
        OR NEW.fuente_geografica_id IS DISTINCT FROM OLD.fuente_geografica_id
        OR NEW.confirmado_por IS DISTINCT FROM OLD.confirmado_por
        OR NEW.confirmado_en IS DISTINCT FROM OLD.confirmado_en
        OR NEW.vigente_hasta IS NULL
        OR NEW.cerrada_por IS NULL THEN
        RAISE EXCEPTION USING ERRCODE = 'P0001',
            MESSAGE = 'agro_ops_contribucion_geografica_historia_inmutable';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER trg_fuentes_geograficas_contribuciones_historia
BEFORE UPDATE OR DELETE ON fuentes_geograficas_contribuciones_canonicas
FOR EACH ROW EXECUTE FUNCTION proteger_historia_contribuciones_geograficas();

CREATE FUNCTION validar_cierre_contribucion_misma_organizacion()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
BEGIN
    IF NEW.cerrada_por IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM public.usuarios
        WHERE id = NEW.cerrada_por AND organizacion_id = NEW.organizacion_id
    ) THEN
        RAISE EXCEPTION USING ERRCODE = 'P0001',
            MESSAGE = 'agro_ops_contribucion_geografica_cierre_organizacion_invalida';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER trg_fuentes_geograficas_contribuciones_cierre_misma_organizacion
BEFORE INSERT OR UPDATE OF organizacion_id, cerrada_por
ON fuentes_geograficas_contribuciones_canonicas
FOR EACH ROW EXECUTE FUNCTION validar_cierre_contribucion_misma_organizacion();

CREATE FUNCTION impedir_truncate_contribuciones_geograficas()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
BEGIN
    RAISE EXCEPTION USING ERRCODE = 'P0001',
        MESSAGE = 'agro_ops_contribucion_geografica_truncate_prohibido';
END;
$$;

CREATE TRIGGER trg_fuentes_geograficas_contribuciones_truncate_prohibido
BEFORE TRUNCATE ON fuentes_geograficas_contribuciones_canonicas
FOR EACH STATEMENT EXECUTE FUNCTION impedir_truncate_contribuciones_geograficas();

CREATE TABLE establecimientos_geometrias_versiones (
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    organizacion_id UUID NOT NULL,
    establecimiento_id UUID NOT NULL,
    version_anterior_id UUID,
    geometria geometry(MultiPolygon, 4326) NOT NULL,
    origen_geometria TEXT NOT NULL,
    fuente_geografica_ids UUID[] NOT NULL DEFAULT ARRAY[]::UUID[],
    creado_por UUID NOT NULL,
    creado_en TIMESTAMPTZ NOT NULL DEFAULT statement_timestamp(),
    CONSTRAINT pk_establecimientos_geometrias_versiones PRIMARY KEY (id),
    CONSTRAINT uq_establecimientos_geometrias_versiones_contexto
        UNIQUE (id, organizacion_id, establecimiento_id),
    CONSTRAINT fk_establecimientos_geometrias_versiones_establecimiento
        FOREIGN KEY (establecimiento_id, organizacion_id)
        REFERENCES establecimientos (id, organizacion_id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    CONSTRAINT fk_establecimientos_geometrias_versiones_anterior
        FOREIGN KEY (version_anterior_id, organizacion_id, establecimiento_id)
        REFERENCES establecimientos_geometrias_versiones (id, organizacion_id, establecimiento_id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    CONSTRAINT fk_establecimientos_geometrias_versiones_creado_por
        FOREIGN KEY (creado_por)
        REFERENCES usuarios (id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    CONSTRAINT ck_establecimientos_geometrias_versiones_geometria_no_vacia
        CHECK (NOT ST_IsEmpty(geometria)),
    CONSTRAINT ck_establecimientos_geometrias_versiones_geometria_valida
        CHECK (ST_IsValid(geometria)),
    CONSTRAINT ck_establecimientos_geometrias_versiones_geometria_srid
        CHECK (ST_SRID(geometria) = 4326),
    CONSTRAINT ck_establecimientos_geometrias_versiones_origen
        CHECK (origen_geometria IN ('senasa_renspa', 'manual', 'importada'))
);

CREATE INDEX idx_establecimientos_geometrias_versiones_historia
    ON establecimientos_geometrias_versiones
    (organizacion_id, establecimiento_id, creado_en, id);

ALTER TABLE establecimientos ADD COLUMN geometria_version_actual_id UUID;

INSERT INTO establecimientos_geometrias_versiones (
    id, organizacion_id, establecimiento_id, geometria, origen_geometria,
    fuente_geografica_ids, creado_por, creado_en
)
SELECT gen_random_uuid(), establecimiento.organizacion_id, establecimiento.id,
       establecimiento.geometria, establecimiento.origen_geometria,
       COALESCE((
           SELECT array_agg(contribucion.fuente_geografica_id ORDER BY contribucion.fuente_geografica_id)
           FROM fuentes_geograficas_contribuciones_canonicas AS contribucion
           WHERE contribucion.organizacion_id = establecimiento.organizacion_id
             AND contribucion.establecimiento_id = establecimiento.id
             AND contribucion.vigente_hasta IS NULL
       ), ARRAY[]::UUID[]),
       establecimiento.creado_por, establecimiento.creado_en
FROM establecimientos AS establecimiento;

UPDATE establecimientos AS establecimiento
SET geometria_version_actual_id = version.id
FROM establecimientos_geometrias_versiones AS version
WHERE version.organizacion_id = establecimiento.organizacion_id
  AND version.establecimiento_id = establecimiento.id;

ALTER TABLE establecimientos
    ADD CONSTRAINT fk_establecimientos_geometria_version_actual
        FOREIGN KEY (geometria_version_actual_id, organizacion_id, id)
        REFERENCES establecimientos_geometrias_versiones (id, organizacion_id, establecimiento_id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT
        DEFERRABLE INITIALLY DEFERRED;

ALTER TABLE unidades_operativas ADD COLUMN establecimiento_geometria_version_id UUID;

UPDATE unidades_operativas AS unidad
SET establecimiento_geometria_version_id = establecimiento.geometria_version_actual_id
FROM establecimientos AS establecimiento
WHERE establecimiento.id = unidad.establecimiento_id
  AND establecimiento.organizacion_id = unidad.organizacion_id;

ALTER TABLE unidades_operativas
    ALTER COLUMN establecimiento_geometria_version_id SET NOT NULL,
    ADD CONSTRAINT fk_unidades_operativas_establecimiento_geometria_version
        FOREIGN KEY (
            establecimiento_geometria_version_id,
            organizacion_id,
            establecimiento_id
        )
        REFERENCES establecimientos_geometrias_versiones (
            id, organizacion_id, establecimiento_id
        )
        ON UPDATE RESTRICT
        ON DELETE RESTRICT;

CREATE FUNCTION asignar_version_geometria_actual_uop()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
BEGIN
    IF NEW.establecimiento_geometria_version_id IS NULL THEN
        SELECT geometria_version_actual_id
        INTO NEW.establecimiento_geometria_version_id
        FROM public.establecimientos
        WHERE id = NEW.establecimiento_id
          AND organizacion_id = NEW.organizacion_id;
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER trg_unidades_operativas_version_geometria_actual
BEFORE INSERT ON unidades_operativas
FOR EACH ROW EXECUTE FUNCTION asignar_version_geometria_actual_uop();

CREATE OR REPLACE FUNCTION proteger_unidad_operativa_territorial()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION USING ERRCODE = 'P0001', MESSAGE = 'agro_ops_uop_eliminacion_prohibida';
    END IF;
    IF NEW.id IS DISTINCT FROM OLD.id
        OR NEW.organizacion_id IS DISTINCT FROM OLD.organizacion_id
        OR NEW.campana_id IS DISTINCT FROM OLD.campana_id
        OR NEW.establecimiento_id IS DISTINCT FROM OLD.establecimiento_id
        OR NEW.establecimiento_geometria_version_id IS DISTINCT FROM OLD.establecimiento_geometria_version_id
        OR NEW.codigo IS DISTINCT FROM OLD.codigo
        OR NEW.nombre IS DISTINCT FROM OLD.nombre
        OR NEW.geometria IS DISTINCT FROM OLD.geometria
        OR NEW.creado_por IS DISTINCT FROM OLD.creado_por
        OR NEW.creado_en IS DISTINCT FROM OLD.creado_en THEN
        RAISE EXCEPTION USING ERRCODE = 'P0001', MESSAGE = 'agro_ops_uop_identidad_inmutable';
    END IF;
    RETURN NEW;
END;
$$;

CREATE FUNCTION proteger_historia_versiones_geometria()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
BEGIN
    RAISE EXCEPTION USING ERRCODE = 'P0001',
        MESSAGE = CASE WHEN TG_OP = 'DELETE'
            THEN 'agro_ops_version_geometria_eliminacion_prohibida'
            ELSE 'agro_ops_version_geometria_inmutable' END;
END;
$$;

CREATE TRIGGER trg_establecimientos_geometrias_versiones_historia
BEFORE UPDATE OR DELETE ON establecimientos_geometrias_versiones
FOR EACH ROW EXECUTE FUNCTION proteger_historia_versiones_geometria();

CREATE TRIGGER trg_establecimientos_geometrias_versiones_truncate_prohibido
BEFORE TRUNCATE ON establecimientos_geometrias_versiones
FOR EACH STATEMENT EXECUTE FUNCTION impedir_truncate_territorio();

CREATE FUNCTION crear_version_geometria_inicial_establecimiento()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
DECLARE
    nueva_version_id UUID;
BEGIN
    nueva_version_id := gen_random_uuid();
    INSERT INTO public.establecimientos_geometrias_versiones (
        id, organizacion_id, establecimiento_id, geometria, origen_geometria,
        fuente_geografica_ids, creado_por, creado_en
    ) VALUES (
        nueva_version_id, NEW.organizacion_id, NEW.id, NEW.geometria,
        NEW.origen_geometria, ARRAY[]::UUID[], NEW.creado_por, NEW.creado_en
    );
    UPDATE public.establecimientos
    SET geometria_version_actual_id = nueva_version_id
    WHERE id = NEW.id AND organizacion_id = NEW.organizacion_id;
    RETURN NEW;
END;
$$;

CREATE TRIGGER trg_establecimientos_version_geometria_inicial
AFTER INSERT ON establecimientos
FOR EACH ROW EXECUTE FUNCTION crear_version_geometria_inicial_establecimiento();

CREATE FUNCTION validar_consistencia_geometria_canonica(establecimiento_validar UUID)
RETURNS VOID
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
DECLARE
    establecimiento_row RECORD;
    geometria_derivada public.geometry;
    fuentes_activas UUID[];
    tiene_historia BOOLEAN;
    version_row RECORD;
BEGIN
    SELECT * INTO establecimiento_row
    FROM public.establecimientos
    WHERE id = establecimiento_validar;
    IF NOT FOUND THEN RETURN; END IF;

    SELECT EXISTS (
        SELECT 1 FROM public.fuentes_geograficas_contribuciones_canonicas
        WHERE establecimiento_id = establecimiento_validar
    ) INTO tiene_historia;
    IF NOT tiene_historia THEN RETURN; END IF;

    SELECT public.ST_Multi(public.ST_UnaryUnion(public.ST_Collect(fuente.geometria))),
           array_agg(fuente.id ORDER BY fuente.id)
    INTO geometria_derivada, fuentes_activas
    FROM public.fuentes_geograficas_contribuciones_canonicas AS contribucion
    JOIN public.fuentes_geograficas AS fuente
      ON fuente.id = contribucion.fuente_geografica_id
     AND fuente.organizacion_id = contribucion.organizacion_id
    WHERE contribucion.establecimiento_id = establecimiento_validar
      AND contribucion.organizacion_id = establecimiento_row.organizacion_id
      AND contribucion.vigente_hasta IS NULL;

    IF geometria_derivada IS NULL OR public.ST_IsEmpty(geometria_derivada)
        OR NOT public.ST_IsValid(geometria_derivada) THEN
        RAISE EXCEPTION USING ERRCODE = 'P0001',
            MESSAGE = 'agro_ops_establecimiento_confirmado_sin_geometria';
    END IF;
    IF NOT public.ST_Equals(establecimiento_row.geometria, geometria_derivada) THEN
        RAISE EXCEPTION USING ERRCODE = 'P0001',
            MESSAGE = 'agro_ops_geometria_canonica_desincronizada';
    END IF;

    SELECT geometria, fuente_geografica_ids INTO version_row
    FROM public.establecimientos_geometrias_versiones
    WHERE id = establecimiento_row.geometria_version_actual_id
      AND organizacion_id = establecimiento_row.organizacion_id
      AND establecimiento_id = establecimiento_validar;
    IF NOT FOUND OR NOT public.ST_Equals(version_row.geometria, geometria_derivada)
        OR version_row.fuente_geografica_ids IS DISTINCT FROM fuentes_activas THEN
        RAISE EXCEPTION USING ERRCODE = 'P0001',
            MESSAGE = 'agro_ops_version_geometria_canonica_desincronizada';
    END IF;
END;
$$;

CREATE FUNCTION validar_consistencia_geometria_canonica_diferida()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
BEGIN
    PERFORM public.validar_consistencia_geometria_canonica(
        COALESCE(NEW.establecimiento_id, OLD.establecimiento_id)
    );
    IF TG_OP = 'UPDATE' AND OLD.establecimiento_id IS DISTINCT FROM NEW.establecimiento_id THEN
        PERFORM public.validar_consistencia_geometria_canonica(OLD.establecimiento_id);
    END IF;
    RETURN COALESCE(NEW, OLD);
END;
$$;

CREATE CONSTRAINT TRIGGER trg_contribuciones_consistencia_canonica_diferida
AFTER INSERT OR UPDATE OR DELETE ON fuentes_geograficas_contribuciones_canonicas
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION validar_consistencia_geometria_canonica_diferida();

CREATE FUNCTION validar_establecimiento_consistencia_canonica_diferida()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
BEGIN
    PERFORM public.validar_consistencia_geometria_canonica(NEW.id);
    RETURN NEW;
END;
$$;

CREATE CONSTRAINT TRIGGER trg_establecimientos_consistencia_canonica_diferida
AFTER UPDATE OF geometria, geometria_version_actual_id ON establecimientos
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION validar_establecimiento_consistencia_canonica_diferida();
