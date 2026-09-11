CREATE TABLE fuentes_geograficas (
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    organizacion_id UUID NOT NULL,
    establecimiento_id UUID NOT NULL,
    external_reference_id UUID NOT NULL,
    tipo_origen TEXT NOT NULL,
    nombre_externo TEXT,
    texto_fuente_original TEXT NOT NULL,
    geometria geometry(MultiPolygon, 4326) NOT NULL,
    huella_sha256 BYTEA NOT NULL,
    version_parser TEXT NOT NULL,
    creado_por UUID NOT NULL,
    creado_en TIMESTAMPTZ NOT NULL DEFAULT statement_timestamp(),
    CONSTRAINT pk_fuentes_geograficas PRIMARY KEY (id),
    CONSTRAINT uq_fuentes_geograficas_organizacion_huella UNIQUE (organizacion_id, huella_sha256),
    CONSTRAINT fk_fuentes_geograficas_organizacion
        FOREIGN KEY (organizacion_id)
        REFERENCES organizaciones (id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    CONSTRAINT fk_fuentes_geograficas_establecimiento_organizacion
        FOREIGN KEY (establecimiento_id, organizacion_id)
        REFERENCES establecimientos (id, organizacion_id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    CONSTRAINT fk_fuentes_geograficas_external_reference
        FOREIGN KEY (external_reference_id)
        REFERENCES external_references (id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    CONSTRAINT fk_fuentes_geograficas_creado_por
        FOREIGN KEY (creado_por)
        REFERENCES usuarios (id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    CONSTRAINT ck_fuentes_geograficas_tipo_origen
        CHECK (tipo_origen = 'senasa_renspa'),
    CONSTRAINT ck_fuentes_geograficas_nombre_externo
        CHECK (
            nombre_externo IS NULL
            OR (
                nombre_externo = btrim(nombre_externo)
                AND nombre_externo <> ''
                AND char_length(nombre_externo) <= 255
            )
        ),
    CONSTRAINT ck_fuentes_geograficas_texto_fuente_original
        CHECK (
            texto_fuente_original <> ''
            AND octet_length(texto_fuente_original) <= 65536
        ),
    CONSTRAINT ck_fuentes_geograficas_geometria_no_vacia
        CHECK (NOT ST_IsEmpty(geometria)),
    CONSTRAINT ck_fuentes_geograficas_geometria_valida
        CHECK (ST_IsValid(geometria)),
    CONSTRAINT ck_fuentes_geograficas_geometria_srid
        CHECK (ST_SRID(geometria) = 4326),
    CONSTRAINT ck_fuentes_geograficas_huella_sha256
        CHECK (octet_length(huella_sha256) = 32),
    CONSTRAINT ck_fuentes_geograficas_version_parser
        CHECK (version_parser = 'senasa_pasted_polygon_v1')
);

CREATE INDEX idx_fuentes_geograficas_establecimiento
    ON fuentes_geograficas (organizacion_id, establecimiento_id, creado_en, id);

CREATE FUNCTION validar_fuente_geografica_external_reference()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
DECLARE
    referencia_organizacion UUID;
    referencia_sistema TEXT;
    referencia_entidad_tipo TEXT;
    referencia_entidad_id UUID;
BEGIN
    SELECT organizacion_id, sistema_externo, entidad_tipo, entidad_id
    INTO referencia_organizacion, referencia_sistema, referencia_entidad_tipo, referencia_entidad_id
    FROM public.external_references
    WHERE id = NEW.external_reference_id;

    IF referencia_organizacion IS DISTINCT FROM NEW.organizacion_id
        OR referencia_sistema IS DISTINCT FROM 'senasa'
        OR referencia_entidad_tipo IS DISTINCT FROM 'establecimiento'
        OR referencia_entidad_id IS DISTINCT FROM NEW.establecimiento_id THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'agro_ops_fuente_geografica_external_reference_invalida';
    END IF;

    RETURN NEW;
END;
$$;

CREATE TRIGGER trg_fuentes_geograficas_external_reference_valida
BEFORE INSERT OR UPDATE OF organizacion_id, establecimiento_id, external_reference_id ON fuentes_geograficas
FOR EACH ROW
EXECUTE FUNCTION validar_fuente_geografica_external_reference();

CREATE TRIGGER trg_fuentes_geograficas_autor_misma_organizacion
BEFORE INSERT OR UPDATE OF organizacion_id, creado_por ON fuentes_geograficas
FOR EACH ROW
EXECUTE FUNCTION validar_autor_territorial_misma_organizacion();

CREATE FUNCTION proteger_historia_fuentes_geograficas()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'agro_ops_fuente_geografica_eliminacion_prohibida';
    END IF;

    RAISE EXCEPTION USING
        ERRCODE = 'P0001',
        MESSAGE = 'agro_ops_fuente_geografica_historia_inmutable';
END;
$$;

CREATE TRIGGER trg_fuentes_geograficas_historia_inmutable
BEFORE UPDATE OR DELETE ON fuentes_geograficas
FOR EACH ROW
EXECUTE FUNCTION proteger_historia_fuentes_geograficas();

CREATE FUNCTION impedir_truncate_fuentes_geograficas()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
BEGIN
    RAISE EXCEPTION USING
        ERRCODE = 'P0001',
        MESSAGE = 'agro_ops_fuente_geografica_truncate_prohibido';
END;
$$;

CREATE TRIGGER trg_fuentes_geograficas_truncate_prohibido
BEFORE TRUNCATE ON fuentes_geograficas
FOR EACH STATEMENT
EXECUTE FUNCTION impedir_truncate_fuentes_geograficas();
