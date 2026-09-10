CREATE TABLE external_references (
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    organizacion_id UUID,
    sistema_externo TEXT NOT NULL,
    external_id TEXT NOT NULL,
    entidad_tipo TEXT NOT NULL,
    entidad_id UUID NOT NULL,
    sync_version TEXT,
    last_sync_status TEXT NOT NULL DEFAULT 'pendiente',
    ultimo_sync_en TIMESTAMPTZ,
    creado_en TIMESTAMPTZ NOT NULL DEFAULT statement_timestamp(),
    actualizado_en TIMESTAMPTZ NOT NULL DEFAULT statement_timestamp(),
    CONSTRAINT pk_external_references PRIMARY KEY (id),
    CONSTRAINT fk_external_references_organizacion
        FOREIGN KEY (organizacion_id)
        REFERENCES organizaciones (id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    CONSTRAINT uq_external_references_identidad_externa
        UNIQUE NULLS NOT DISTINCT (organizacion_id, sistema_externo, external_id),
    CONSTRAINT ck_external_references_sistema_externo
        CHECK (
            char_length(sistema_externo) BETWEEN 1 AND 64
            AND sistema_externo ~ '^[a-z][a-z0-9_]*$'
        ),
    CONSTRAINT ck_external_references_external_id
        CHECK (
            external_id = btrim(external_id)
            AND external_id <> ''
            AND char_length(external_id) <= 256
        ),
    CONSTRAINT ck_external_references_entidad_tipo
        CHECK (
            char_length(entidad_tipo) BETWEEN 1 AND 128
            AND entidad_tipo ~ '^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)*$'
        ),
    CONSTRAINT ck_external_references_sync_version
        CHECK (
            sync_version IS NULL
            OR (
                sync_version = btrim(sync_version)
                AND sync_version <> ''
                AND char_length(sync_version) <= 256
            )
        ),
    CONSTRAINT ck_external_references_last_sync_status
        CHECK (last_sync_status IN ('pendiente', 'sincronizado', 'fallido', 'desactualizado'))
);

CREATE INDEX idx_external_references_entidad
    ON external_references (organizacion_id, entidad_tipo, entidad_id, id);

CREATE FUNCTION proteger_identidad_external_reference()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
BEGIN
    IF NEW.organizacion_id IS DISTINCT FROM OLD.organizacion_id
        OR NEW.sistema_externo IS DISTINCT FROM OLD.sistema_externo
        OR NEW.external_id IS DISTINCT FROM OLD.external_id
        OR NEW.entidad_tipo IS DISTINCT FROM OLD.entidad_tipo
        OR NEW.entidad_id IS DISTINCT FROM OLD.entidad_id THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'agro_ops_external_reference_identidad_inmutable';
    END IF;

    RETURN NEW;
END;
$$;

CREATE TRIGGER trg_external_references_identidad_inmutable
BEFORE UPDATE ON external_references
FOR EACH ROW
EXECUTE FUNCTION proteger_identidad_external_reference();
