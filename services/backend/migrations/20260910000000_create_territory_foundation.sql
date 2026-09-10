CREATE TABLE establecimientos (
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    organizacion_id UUID NOT NULL,
    codigo TEXT NOT NULL,
    nombre TEXT NOT NULL,
    geometria geometry(MultiPolygon, 4326) NOT NULL,
    origen_geometria TEXT NOT NULL,
    activo BOOLEAN NOT NULL DEFAULT TRUE,
    creado_por UUID NOT NULL,
    creado_en TIMESTAMPTZ NOT NULL DEFAULT statement_timestamp(),
    CONSTRAINT pk_establecimientos PRIMARY KEY (id),
    CONSTRAINT uq_establecimientos_id_organizacion UNIQUE (id, organizacion_id),
    CONSTRAINT uq_establecimientos_organizacion_codigo UNIQUE (organizacion_id, codigo),
    CONSTRAINT fk_establecimientos_organizacion
        FOREIGN KEY (organizacion_id)
        REFERENCES organizaciones (id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    CONSTRAINT fk_establecimientos_creado_por
        FOREIGN KEY (creado_por)
        REFERENCES usuarios (id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    CONSTRAINT ck_establecimientos_codigo
        CHECK (
            char_length(codigo) BETWEEN 1 AND 64
            AND codigo ~ '^[A-Z][A-Z0-9_-]*$'
        ),
    CONSTRAINT ck_establecimientos_nombre
        CHECK (
            char_length(nombre) BETWEEN 1 AND 255
            AND nombre = btrim(nombre)
        ),
    CONSTRAINT ck_establecimientos_geometria_no_vacia
        CHECK (NOT ST_IsEmpty(geometria)),
    CONSTRAINT ck_establecimientos_geometria_valida
        CHECK (ST_IsValid(geometria)),
    CONSTRAINT ck_establecimientos_geometria_srid
        CHECK (ST_SRID(geometria) = 4326),
    CONSTRAINT ck_establecimientos_origen_geometria
        CHECK (origen_geometria IN ('senasa_renspa', 'manual', 'importada'))
);

CREATE INDEX idx_establecimientos_geometria
    ON establecimientos
    USING GIST (geometria);

CREATE TABLE lotes_base (
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    organizacion_id UUID NOT NULL,
    establecimiento_id UUID NOT NULL,
    codigo TEXT NOT NULL,
    nombre TEXT NOT NULL,
    geometria geometry(MultiPolygon, 4326) NOT NULL,
    activo BOOLEAN NOT NULL DEFAULT TRUE,
    creado_por UUID NOT NULL,
    creado_en TIMESTAMPTZ NOT NULL DEFAULT statement_timestamp(),
    CONSTRAINT pk_lotes_base PRIMARY KEY (id),
    CONSTRAINT uq_lotes_base_establecimiento_codigo
        UNIQUE (establecimiento_id, codigo),
    CONSTRAINT fk_lotes_base_organizacion
        FOREIGN KEY (organizacion_id)
        REFERENCES organizaciones (id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    CONSTRAINT fk_lotes_base_establecimiento_organizacion
        FOREIGN KEY (establecimiento_id, organizacion_id)
        REFERENCES establecimientos (id, organizacion_id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    CONSTRAINT fk_lotes_base_creado_por
        FOREIGN KEY (creado_por)
        REFERENCES usuarios (id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    CONSTRAINT ck_lotes_base_codigo
        CHECK (
            char_length(codigo) BETWEEN 1 AND 64
            AND codigo ~ '^[A-Z][A-Z0-9_-]*$'
        ),
    CONSTRAINT ck_lotes_base_nombre
        CHECK (
            char_length(nombre) BETWEEN 1 AND 255
            AND nombre = btrim(nombre)
        ),
    CONSTRAINT ck_lotes_base_geometria_no_vacia
        CHECK (NOT ST_IsEmpty(geometria)),
    CONSTRAINT ck_lotes_base_geometria_valida
        CHECK (ST_IsValid(geometria)),
    CONSTRAINT ck_lotes_base_geometria_srid
        CHECK (ST_SRID(geometria) = 4326)
);

CREATE INDEX idx_lotes_base_geometria
    ON lotes_base
    USING GIST (geometria);

CREATE FUNCTION validar_autor_territorial_misma_organizacion()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
DECLARE
    organizacion_autor UUID;
BEGIN
    SELECT organizacion_id
    INTO organizacion_autor
    FROM public.usuarios
    WHERE id = NEW.creado_por;

    IF organizacion_autor IS DISTINCT FROM NEW.organizacion_id THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'agro_ops_territorio_autor_organizacion_invalida';
    END IF;

    RETURN NEW;
END;
$$;

CREATE TRIGGER trg_establecimientos_autor_misma_organizacion
BEFORE INSERT OR UPDATE OF organizacion_id, creado_por ON establecimientos
FOR EACH ROW
EXECUTE FUNCTION validar_autor_territorial_misma_organizacion();

CREATE TRIGGER trg_lotes_base_autor_misma_organizacion
BEFORE INSERT OR UPDATE OF organizacion_id, creado_por ON lotes_base
FOR EACH ROW
EXECUTE FUNCTION validar_autor_territorial_misma_organizacion();

CREATE FUNCTION validar_lote_base_contenido_en_establecimiento()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
DECLARE
    geometria_establecimiento public.geometry;
BEGIN
    IF NEW.geometria IS NULL
        OR public.ST_IsEmpty(NEW.geometria)
        OR NOT public.ST_IsValid(NEW.geometria) THEN
        RETURN NEW;
    END IF;

    SELECT geometria
    INTO geometria_establecimiento
    FROM public.establecimientos
    WHERE id = NEW.establecimiento_id
      AND organizacion_id = NEW.organizacion_id;

    IF NOT public.ST_CoveredBy(NEW.geometria, geometria_establecimiento) THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'agro_ops_lote_base_fuera_establecimiento';
    END IF;

    RETURN NEW;
END;
$$;

CREATE TRIGGER trg_lotes_base_contenido_en_establecimiento
BEFORE INSERT OR UPDATE OF organizacion_id, establecimiento_id, geometria ON lotes_base
FOR EACH ROW
EXECUTE FUNCTION validar_lote_base_contenido_en_establecimiento();

CREATE FUNCTION validar_establecimiento_cubre_lotes_base()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
BEGIN
    IF NEW.geometria IS NULL
        OR public.ST_IsEmpty(NEW.geometria)
        OR NOT public.ST_IsValid(NEW.geometria) THEN
        RETURN NEW;
    END IF;

    IF EXISTS (
        SELECT 1
        FROM public.lotes_base AS lote_base
        WHERE lote_base.establecimiento_id = NEW.id
          AND lote_base.organizacion_id = NEW.organizacion_id
          AND NOT public.ST_CoveredBy(lote_base.geometria, NEW.geometria)
    ) THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'agro_ops_establecimiento_no_cubre_lotes_base';
    END IF;

    RETURN NEW;
END;
$$;

CREATE TRIGGER trg_establecimientos_cubren_lotes_base
BEFORE UPDATE OF geometria ON establecimientos
FOR EACH ROW
EXECUTE FUNCTION validar_establecimiento_cubre_lotes_base();

CREATE FUNCTION proteger_establecimiento_territorial()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'agro_ops_establecimiento_eliminacion_prohibida';
    END IF;

    IF NEW.id IS DISTINCT FROM OLD.id
        OR NEW.organizacion_id IS DISTINCT FROM OLD.organizacion_id
        OR NEW.codigo IS DISTINCT FROM OLD.codigo
        OR NEW.nombre IS DISTINCT FROM OLD.nombre
        OR NEW.creado_por IS DISTINCT FROM OLD.creado_por
        OR NEW.creado_en IS DISTINCT FROM OLD.creado_en THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'agro_ops_establecimiento_identidad_inmutable';
    END IF;

    RETURN NEW;
END;
$$;

CREATE FUNCTION proteger_lote_base_territorial()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'agro_ops_lote_base_eliminacion_prohibida';
    END IF;

    IF NEW.id IS DISTINCT FROM OLD.id
        OR NEW.organizacion_id IS DISTINCT FROM OLD.organizacion_id
        OR NEW.establecimiento_id IS DISTINCT FROM OLD.establecimiento_id
        OR NEW.codigo IS DISTINCT FROM OLD.codigo
        OR NEW.nombre IS DISTINCT FROM OLD.nombre
        OR NEW.creado_por IS DISTINCT FROM OLD.creado_por
        OR NEW.creado_en IS DISTINCT FROM OLD.creado_en THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'agro_ops_lote_base_identidad_inmutable';
    END IF;

    RETURN NEW;
END;
$$;

CREATE TRIGGER trg_establecimientos_identidad
BEFORE UPDATE OR DELETE ON establecimientos
FOR EACH ROW
EXECUTE FUNCTION proteger_establecimiento_territorial();

CREATE TRIGGER trg_lotes_base_identidad
BEFORE UPDATE OR DELETE ON lotes_base
FOR EACH ROW
EXECUTE FUNCTION proteger_lote_base_territorial();

CREATE FUNCTION impedir_truncate_territorio()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
BEGIN
    RAISE EXCEPTION USING
        ERRCODE = 'P0001',
        MESSAGE = 'agro_ops_territorio_truncate_prohibido';
END;
$$;

CREATE TRIGGER trg_establecimientos_truncate_prohibido
BEFORE TRUNCATE ON establecimientos
FOR EACH STATEMENT
EXECUTE FUNCTION impedir_truncate_territorio();

CREATE TRIGGER trg_lotes_base_truncate_prohibido
BEFORE TRUNCATE ON lotes_base
FOR EACH STATEMENT
EXECUTE FUNCTION impedir_truncate_territorio();
