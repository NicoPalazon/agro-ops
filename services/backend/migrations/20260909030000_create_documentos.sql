CREATE TABLE documentos (
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    organizacion_id UUID NOT NULL,
    nombre_original TEXT NOT NULL,
    tipo_mime TEXT NOT NULL,
    tamano_bytes BIGINT NOT NULL,
    sha256 BYTEA NOT NULL,
    storage_bucket TEXT NOT NULL,
    storage_key TEXT NOT NULL,
    creado_por UUID NOT NULL,
    creado_en TIMESTAMPTZ NOT NULL DEFAULT statement_timestamp(),
    CONSTRAINT pk_documentos PRIMARY KEY (id),
    CONSTRAINT fk_documentos_organizacion
        FOREIGN KEY (organizacion_id)
        REFERENCES organizaciones (id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    CONSTRAINT fk_documentos_creado_por
        FOREIGN KEY (creado_por)
        REFERENCES usuarios (id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    CONSTRAINT uq_documentos_storage_key UNIQUE (storage_key),
    CONSTRAINT ck_documentos_nombre_original
        CHECK (
            nombre_original = btrim(nombre_original)
            AND nombre_original <> ''
            AND char_length(nombre_original) <= 255
        ),
    CONSTRAINT ck_documentos_tipo_mime
        CHECK (
            tipo_mime = btrim(tipo_mime)
            AND tipo_mime <> ''
            AND char_length(tipo_mime) <= 255
        ),
    CONSTRAINT ck_documentos_tamano_bytes CHECK (tamano_bytes > 0),
    CONSTRAINT ck_documentos_sha256 CHECK (octet_length(sha256) = 32),
    CONSTRAINT ck_documentos_storage_bucket
        CHECK (
            char_length(storage_bucket) BETWEEN 1 AND 63
            AND storage_bucket ~ '^[a-z][a-z0-9_-]*$'
        ),
    CONSTRAINT ck_documentos_storage_key
        CHECK (
            char_length(storage_key) BETWEEN 1 AND 255
            AND storage_key = btrim(storage_key)
            AND storage_key ~ '^organizaciones/[0-9a-f-]{36}/documentos/[0-9a-f-]{36}/contenido$'
        )
);

CREATE INDEX idx_documentos_organizacion_sha256
    ON documentos (organizacion_id, sha256);

CREATE FUNCTION validar_documento_creador_misma_organizacion()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
DECLARE
    organizacion_creador UUID;
BEGIN
    SELECT organizacion_id
    INTO organizacion_creador
    FROM public.usuarios
    WHERE id = NEW.creado_por;

    IF NOT FOUND THEN
        RETURN NEW;
    END IF;

    IF organizacion_creador IS DISTINCT FROM NEW.organizacion_id THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'agro_ops_documento_creador_organizacion_invalida';
    END IF;

    RETURN NEW;
END;
$$;

CREATE TRIGGER trg_documentos_creador_misma_organizacion
BEFORE INSERT OR UPDATE OF organizacion_id, creado_por ON documentos
FOR EACH ROW
EXECUTE FUNCTION validar_documento_creador_misma_organizacion();
