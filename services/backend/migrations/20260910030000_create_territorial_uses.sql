CREATE EXTENSION IF NOT EXISTS btree_gist;

CREATE TABLE usos_territoriales (
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    organizacion_id UUID NOT NULL,
    codigo TEXT NOT NULL,
    nombre TEXT NOT NULL,
    activo BOOLEAN NOT NULL DEFAULT TRUE,
    creado_por UUID NOT NULL,
    creado_en TIMESTAMPTZ NOT NULL DEFAULT statement_timestamp(),
    CONSTRAINT pk_usos_territoriales PRIMARY KEY (id),
    CONSTRAINT uq_usos_territoriales_id_organizacion UNIQUE (id, organizacion_id),
    CONSTRAINT uq_usos_territoriales_organizacion_codigo UNIQUE (organizacion_id, codigo),
    CONSTRAINT fk_usos_territoriales_organizacion
        FOREIGN KEY (organizacion_id)
        REFERENCES organizaciones (id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    CONSTRAINT fk_usos_territoriales_creado_por
        FOREIGN KEY (creado_por)
        REFERENCES usuarios (id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    CONSTRAINT ck_usos_territoriales_codigo
        CHECK (
            char_length(codigo) BETWEEN 1 AND 64
            AND codigo ~ '^[A-Z][A-Z0-9_-]*$'
        ),
    CONSTRAINT ck_usos_territoriales_nombre
        CHECK (
            char_length(nombre) BETWEEN 1 AND 255
            AND nombre = btrim(nombre)
        )
);

CREATE TABLE unidades_operativas_usos (
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    unidad_operativa_id UUID NOT NULL,
    uso_territorial_id UUID NOT NULL,
    fecha_inicio DATE NOT NULL,
    fecha_fin DATE NOT NULL,
    creado_por UUID NOT NULL,
    creado_en TIMESTAMPTZ NOT NULL DEFAULT statement_timestamp(),
    CONSTRAINT pk_unidades_operativas_usos PRIMARY KEY (id),
    CONSTRAINT fk_uop_usos_unidad_operativa
        FOREIGN KEY (unidad_operativa_id)
        REFERENCES unidades_operativas (id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    CONSTRAINT fk_uop_usos_uso_territorial
        FOREIGN KEY (uso_territorial_id)
        REFERENCES usos_territoriales (id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    CONSTRAINT fk_uop_usos_creado_por
        FOREIGN KEY (creado_por)
        REFERENCES usuarios (id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    CONSTRAINT ck_uop_usos_periodo_no_vacio
        CHECK (fecha_fin > fecha_inicio),
    CONSTRAINT ex_uop_usos_sin_solapamiento
        EXCLUDE USING GIST (
            unidad_operativa_id WITH =,
            daterange(fecha_inicio, fecha_fin, '[)') WITH &&
        )
);

CREATE INDEX idx_unidades_operativas_usos_uso
    ON unidades_operativas_usos (uso_territorial_id, unidad_operativa_id);

CREATE TRIGGER trg_usos_territoriales_autor_misma_organizacion
BEFORE INSERT OR UPDATE OF organizacion_id, creado_por ON usos_territoriales
FOR EACH ROW
EXECUTE FUNCTION validar_autor_territorial_misma_organizacion();

CREATE FUNCTION validar_contexto_unidad_operativa_uso()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
DECLARE
    organizacion_unidad UUID;
    fecha_inicio_campana DATE;
    fecha_fin_campana DATE;
    organizacion_uso UUID;
    organizacion_autor UUID;
BEGIN
    SELECT unidad.organizacion_id, campana.fecha_inicio, campana.fecha_fin
    INTO organizacion_unidad, fecha_inicio_campana, fecha_fin_campana
    FROM public.unidades_operativas AS unidad
    JOIN public.campanas AS campana
      ON campana.id = unidad.campana_id
     AND campana.organizacion_id = unidad.organizacion_id
    WHERE unidad.id = NEW.unidad_operativa_id
    FOR KEY SHARE OF unidad, campana;

    SELECT organizacion_id
    INTO organizacion_uso
    FROM public.usos_territoriales
    WHERE id = NEW.uso_territorial_id
    FOR KEY SHARE;

    SELECT organizacion_id
    INTO organizacion_autor
    FROM public.usuarios
    WHERE id = NEW.creado_por;

    IF organizacion_unidad IS DISTINCT FROM organizacion_uso THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'agro_ops_uop_uso_organizacion_invalida';
    END IF;

    IF organizacion_autor IS DISTINCT FROM organizacion_unidad THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'agro_ops_uop_uso_autor_organizacion_invalida';
    END IF;

    IF NEW.fecha_inicio < fecha_inicio_campana
        OR NEW.fecha_fin > fecha_fin_campana + 1 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'agro_ops_uop_uso_fuera_campana';
    END IF;

    RETURN NEW;
END;
$$;

CREATE TRIGGER trg_uop_usos_contexto
BEFORE INSERT OR UPDATE OF unidad_operativa_id, uso_territorial_id, fecha_inicio, fecha_fin, creado_por
ON unidades_operativas_usos
FOR EACH ROW
EXECUTE FUNCTION validar_contexto_unidad_operativa_uso();

CREATE FUNCTION proteger_uso_territorial()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'agro_ops_uso_territorial_eliminacion_prohibida';
    END IF;

    IF NEW.id IS DISTINCT FROM OLD.id
        OR NEW.organizacion_id IS DISTINCT FROM OLD.organizacion_id
        OR NEW.codigo IS DISTINCT FROM OLD.codigo
        OR NEW.nombre IS DISTINCT FROM OLD.nombre
        OR NEW.creado_por IS DISTINCT FROM OLD.creado_por
        OR NEW.creado_en IS DISTINCT FROM OLD.creado_en THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'agro_ops_uso_territorial_identidad_inmutable';
    END IF;

    RETURN NEW;
END;
$$;

CREATE TRIGGER trg_usos_territoriales_identidad
BEFORE UPDATE OR DELETE ON usos_territoriales
FOR EACH ROW
EXECUTE FUNCTION proteger_uso_territorial();

CREATE FUNCTION proteger_unidad_operativa_uso()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'agro_ops_uop_uso_eliminacion_prohibida';
    END IF;

    RAISE EXCEPTION USING
        ERRCODE = 'P0001',
        MESSAGE = 'agro_ops_uop_uso_inmutable';
END;
$$;

CREATE TRIGGER trg_uop_usos_historia
BEFORE UPDATE OR DELETE ON unidades_operativas_usos
FOR EACH ROW
EXECUTE FUNCTION proteger_unidad_operativa_uso();

CREATE TRIGGER trg_usos_territoriales_truncate_prohibido
BEFORE TRUNCATE ON usos_territoriales
FOR EACH STATEMENT
EXECUTE FUNCTION impedir_truncate_territorio();

CREATE TRIGGER trg_uop_usos_truncate_prohibido
BEFORE TRUNCATE ON unidades_operativas_usos
FOR EACH STATEMENT
EXECUTE FUNCTION impedir_truncate_territorio();
