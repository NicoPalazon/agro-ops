CREATE TABLE campanas (
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    organizacion_id UUID NOT NULL,
    codigo TEXT NOT NULL,
    nombre TEXT NOT NULL,
    fecha_inicio DATE NOT NULL,
    fecha_fin DATE NOT NULL,
    activa BOOLEAN NOT NULL DEFAULT TRUE,
    creado_por UUID NOT NULL,
    creado_en TIMESTAMPTZ NOT NULL DEFAULT statement_timestamp(),
    CONSTRAINT pk_campanas PRIMARY KEY (id),
    CONSTRAINT uq_campanas_id_organizacion UNIQUE (id, organizacion_id),
    CONSTRAINT uq_campanas_organizacion_codigo UNIQUE (organizacion_id, codigo),
    CONSTRAINT fk_campanas_organizacion
        FOREIGN KEY (organizacion_id)
        REFERENCES organizaciones (id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    CONSTRAINT fk_campanas_creado_por
        FOREIGN KEY (creado_por)
        REFERENCES usuarios (id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    CONSTRAINT ck_campanas_codigo
        CHECK (
            char_length(codigo) BETWEEN 1 AND 64
            AND codigo ~ '^[A-Z][A-Z0-9_-]*$'
        ),
    CONSTRAINT ck_campanas_nombre
        CHECK (
            char_length(nombre) BETWEEN 1 AND 255
            AND nombre = btrim(nombre)
        ),
    CONSTRAINT ck_campanas_periodo
        CHECK (fecha_fin >= fecha_inicio)
);

CREATE TABLE unidades_operativas (
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    organizacion_id UUID NOT NULL,
    campana_id UUID NOT NULL,
    establecimiento_id UUID NOT NULL,
    codigo TEXT NOT NULL,
    nombre TEXT NOT NULL,
    geometria geometry(MultiPolygon, 4326) NOT NULL,
    activa BOOLEAN NOT NULL DEFAULT TRUE,
    creado_por UUID NOT NULL,
    creado_en TIMESTAMPTZ NOT NULL DEFAULT statement_timestamp(),
    CONSTRAINT pk_unidades_operativas PRIMARY KEY (id),
    CONSTRAINT uq_unidades_operativas_id_contexto
        UNIQUE (id, organizacion_id, establecimiento_id),
    CONSTRAINT uq_unidades_operativas_campana_establecimiento_codigo
        UNIQUE (campana_id, establecimiento_id, codigo),
    CONSTRAINT fk_unidades_operativas_campana_organizacion
        FOREIGN KEY (campana_id, organizacion_id)
        REFERENCES campanas (id, organizacion_id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    CONSTRAINT fk_unidades_operativas_establecimiento_organizacion
        FOREIGN KEY (establecimiento_id, organizacion_id)
        REFERENCES establecimientos (id, organizacion_id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    CONSTRAINT fk_unidades_operativas_creado_por
        FOREIGN KEY (creado_por)
        REFERENCES usuarios (id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    CONSTRAINT ck_unidades_operativas_codigo
        CHECK (
            char_length(codigo) BETWEEN 1 AND 64
            AND codigo ~ '^[A-Z][A-Z0-9_-]*$'
        ),
    CONSTRAINT ck_unidades_operativas_nombre
        CHECK (
            char_length(nombre) BETWEEN 1 AND 255
            AND nombre = btrim(nombre)
        ),
    CONSTRAINT ck_unidades_operativas_geometria_no_vacia
        CHECK (NOT ST_IsEmpty(geometria)),
    CONSTRAINT ck_unidades_operativas_geometria_valida
        CHECK (ST_IsValid(geometria)),
    CONSTRAINT ck_unidades_operativas_geometria_srid
        CHECK (ST_SRID(geometria) = 4326)
);

CREATE INDEX idx_unidades_operativas_geometria
    ON unidades_operativas
    USING GIST (geometria);

ALTER TABLE lotes_base
    ADD CONSTRAINT uq_lotes_base_id_contexto
    UNIQUE (id, organizacion_id, establecimiento_id);

CREATE TABLE unidades_operativas_lotes_base (
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    organizacion_id UUID NOT NULL,
    establecimiento_id UUID NOT NULL,
    unidad_operativa_id UUID NOT NULL,
    lote_base_id UUID NOT NULL,
    creado_por UUID NOT NULL,
    creado_en TIMESTAMPTZ NOT NULL DEFAULT statement_timestamp(),
    CONSTRAINT pk_unidades_operativas_lotes_base PRIMARY KEY (id),
    CONSTRAINT uq_unidades_operativas_lotes_base_par
        UNIQUE (unidad_operativa_id, lote_base_id),
    CONSTRAINT fk_uop_lotes_base_unidad_operativa_contexto
        FOREIGN KEY (unidad_operativa_id, organizacion_id, establecimiento_id)
        REFERENCES unidades_operativas (id, organizacion_id, establecimiento_id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    CONSTRAINT fk_uop_lotes_base_lote_base_contexto
        FOREIGN KEY (lote_base_id, organizacion_id, establecimiento_id)
        REFERENCES lotes_base (id, organizacion_id, establecimiento_id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    CONSTRAINT fk_uop_lotes_base_creado_por
        FOREIGN KEY (creado_por)
        REFERENCES usuarios (id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT
);

CREATE INDEX idx_unidades_operativas_lotes_base_lote
    ON unidades_operativas_lotes_base (lote_base_id, unidad_operativa_id);

CREATE TRIGGER trg_campanas_autor_misma_organizacion
BEFORE INSERT OR UPDATE OF organizacion_id, creado_por ON campanas
FOR EACH ROW
EXECUTE FUNCTION validar_autor_territorial_misma_organizacion();

CREATE TRIGGER trg_unidades_operativas_autor_misma_organizacion
BEFORE INSERT OR UPDATE OF organizacion_id, creado_por ON unidades_operativas
FOR EACH ROW
EXECUTE FUNCTION validar_autor_territorial_misma_organizacion();

CREATE TRIGGER trg_uop_lotes_base_autor_misma_organizacion
BEFORE INSERT OR UPDATE OF organizacion_id, creado_por ON unidades_operativas_lotes_base
FOR EACH ROW
EXECUTE FUNCTION validar_autor_territorial_misma_organizacion();

CREATE FUNCTION bloquear_campana_unidad_operativa()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
BEGIN
    PERFORM 1
    FROM public.campanas
    WHERE id = NEW.campana_id
    FOR UPDATE;

    RETURN NEW;
END;
$$;

CREATE TRIGGER trg_unidades_operativas_bloquear_campana
BEFORE INSERT OR UPDATE ON unidades_operativas
FOR EACH ROW
EXECUTE FUNCTION bloquear_campana_unidad_operativa();

CREATE FUNCTION bloquear_contexto_uop_lote_base()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
BEGIN
    PERFORM 1
    FROM public.unidades_operativas
    WHERE id = NEW.unidad_operativa_id
    FOR UPDATE;

    PERFORM 1
    FROM public.lotes_base
    WHERE id = NEW.lote_base_id
    FOR SHARE;

    RETURN NEW;
END;
$$;

CREATE TRIGGER trg_uop_lotes_base_bloquear_contexto
BEFORE INSERT OR UPDATE ON unidades_operativas_lotes_base
FOR EACH ROW
EXECUTE FUNCTION bloquear_contexto_uop_lote_base();

CREATE FUNCTION validar_integridad_unidad_operativa(unidad_id UUID)
RETURNS VOID
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
DECLARE
    organizacion_uop UUID;
    campana_uop UUID;
    establecimiento_uop UUID;
    geometria_uop public.geometry;
    geometria_lotes_base public.geometry;
BEGIN
    SELECT organizacion_id, campana_id, establecimiento_id, geometria
    INTO organizacion_uop, campana_uop, establecimiento_uop, geometria_uop
    FROM public.unidades_operativas
    WHERE id = unidad_id;

    IF NOT FOUND THEN
        RETURN;
    END IF;

    SELECT public.ST_UnaryUnion(public.ST_Collect(lote_base.geometria))
    INTO geometria_lotes_base
    FROM public.unidades_operativas_lotes_base AS vinculo
    JOIN public.lotes_base AS lote_base
      ON lote_base.id = vinculo.lote_base_id
     AND lote_base.organizacion_id = vinculo.organizacion_id
     AND lote_base.establecimiento_id = vinculo.establecimiento_id
    WHERE vinculo.unidad_operativa_id = unidad_id;

    IF geometria_lotes_base IS NULL THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'agro_ops_uop_sin_lotes_base';
    END IF;

    IF NOT public.ST_CoveredBy(geometria_uop, geometria_lotes_base) THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'agro_ops_uop_fuera_lotes_base';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM public.unidades_operativas AS otra
        WHERE otra.id <> unidad_id
          AND otra.organizacion_id = organizacion_uop
          AND otra.campana_id = campana_uop
          AND otra.establecimiento_id = establecimiento_uop
          AND otra.geometria OPERATOR(public.&&) geometria_uop
          AND public.ST_Relate(otra.geometria, geometria_uop, '2********')
    ) THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'agro_ops_uop_solapamiento_interior';
    END IF;
END;
$$;

CREATE FUNCTION validar_unidad_operativa_diferida()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
BEGIN
    PERFORM public.validar_integridad_unidad_operativa(COALESCE(NEW.id, OLD.id));
    RETURN COALESCE(NEW, OLD);
END;
$$;

CREATE CONSTRAINT TRIGGER trg_unidades_operativas_integridad_diferida
AFTER INSERT OR UPDATE OR DELETE ON unidades_operativas
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW
EXECUTE FUNCTION validar_unidad_operativa_diferida();

CREATE FUNCTION validar_vinculo_uop_lote_base_diferido()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
BEGIN
    PERFORM public.validar_integridad_unidad_operativa(
        COALESCE(NEW.unidad_operativa_id, OLD.unidad_operativa_id)
    );
    RETURN COALESCE(NEW, OLD);
END;
$$;

CREATE CONSTRAINT TRIGGER trg_uop_lotes_base_integridad_diferida
AFTER INSERT OR UPDATE OR DELETE ON unidades_operativas_lotes_base
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW
EXECUTE FUNCTION validar_vinculo_uop_lote_base_diferido();

CREATE FUNCTION validar_lote_base_uops_diferido()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
DECLARE
    unidad_id UUID;
BEGIN
    FOR unidad_id IN
        SELECT vinculo.unidad_operativa_id
        FROM public.unidades_operativas_lotes_base AS vinculo
        WHERE vinculo.lote_base_id = NEW.id
    LOOP
        PERFORM public.validar_integridad_unidad_operativa(unidad_id);
    END LOOP;

    RETURN NEW;
END;
$$;

CREATE CONSTRAINT TRIGGER trg_lotes_base_uops_integridad_diferida
AFTER UPDATE ON lotes_base
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW
EXECUTE FUNCTION validar_lote_base_uops_diferido();

CREATE FUNCTION proteger_campana_territorial()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'agro_ops_campana_eliminacion_prohibida';
    END IF;

    IF NEW.id IS DISTINCT FROM OLD.id
        OR NEW.organizacion_id IS DISTINCT FROM OLD.organizacion_id
        OR NEW.codigo IS DISTINCT FROM OLD.codigo
        OR NEW.nombre IS DISTINCT FROM OLD.nombre
        OR NEW.fecha_inicio IS DISTINCT FROM OLD.fecha_inicio
        OR NEW.fecha_fin IS DISTINCT FROM OLD.fecha_fin
        OR NEW.creado_por IS DISTINCT FROM OLD.creado_por
        OR NEW.creado_en IS DISTINCT FROM OLD.creado_en THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'agro_ops_campana_identidad_inmutable';
    END IF;

    RETURN NEW;
END;
$$;

CREATE TRIGGER trg_campanas_identidad
BEFORE UPDATE OR DELETE ON campanas
FOR EACH ROW
EXECUTE FUNCTION proteger_campana_territorial();

CREATE FUNCTION proteger_unidad_operativa_territorial()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'agro_ops_uop_eliminacion_prohibida';
    END IF;

    IF NEW.id IS DISTINCT FROM OLD.id
        OR NEW.organizacion_id IS DISTINCT FROM OLD.organizacion_id
        OR NEW.campana_id IS DISTINCT FROM OLD.campana_id
        OR NEW.establecimiento_id IS DISTINCT FROM OLD.establecimiento_id
        OR NEW.codigo IS DISTINCT FROM OLD.codigo
        OR NEW.nombre IS DISTINCT FROM OLD.nombre
        OR NEW.geometria IS DISTINCT FROM OLD.geometria
        OR NEW.creado_por IS DISTINCT FROM OLD.creado_por
        OR NEW.creado_en IS DISTINCT FROM OLD.creado_en THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'agro_ops_uop_identidad_inmutable';
    END IF;

    RETURN NEW;
END;
$$;

CREATE TRIGGER trg_unidades_operativas_identidad
BEFORE UPDATE OR DELETE ON unidades_operativas
FOR EACH ROW
EXECUTE FUNCTION proteger_unidad_operativa_territorial();

CREATE FUNCTION proteger_vinculo_uop_lote_base()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'agro_ops_uop_lote_base_eliminacion_prohibida';
    END IF;

    RAISE EXCEPTION USING
        ERRCODE = 'P0001',
        MESSAGE = 'agro_ops_uop_lote_base_inmutable';
END;
$$;

CREATE TRIGGER trg_uop_lotes_base_historia
BEFORE UPDATE OR DELETE ON unidades_operativas_lotes_base
FOR EACH ROW
EXECUTE FUNCTION proteger_vinculo_uop_lote_base();

CREATE FUNCTION impedir_truncate_campanas_uops()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
BEGIN
    RAISE EXCEPTION USING
        ERRCODE = 'P0001',
        MESSAGE = 'agro_ops_campanas_uops_truncate_prohibido';
END;
$$;

CREATE TRIGGER trg_campanas_truncate_prohibido
BEFORE TRUNCATE ON campanas
FOR EACH STATEMENT
EXECUTE FUNCTION impedir_truncate_campanas_uops();

CREATE TRIGGER trg_unidades_operativas_truncate_prohibido
BEFORE TRUNCATE ON unidades_operativas
FOR EACH STATEMENT
EXECUTE FUNCTION impedir_truncate_campanas_uops();

CREATE TRIGGER trg_uop_lotes_base_truncate_prohibido
BEFORE TRUNCATE ON unidades_operativas_lotes_base
FOR EACH STATEMENT
EXECUTE FUNCTION impedir_truncate_campanas_uops();
