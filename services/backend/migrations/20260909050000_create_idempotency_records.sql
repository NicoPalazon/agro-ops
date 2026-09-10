CREATE TABLE idempotency_records (
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    organizacion_id UUID,
    operacion TEXT NOT NULL,
    idempotency_key TEXT NOT NULL,
    request_sha256 BYTEA NOT NULL,
    resultado JSONB NOT NULL,
    completado_en TIMESTAMPTZ NOT NULL DEFAULT statement_timestamp(),
    CONSTRAINT pk_idempotency_records PRIMARY KEY (id),
    CONSTRAINT fk_idempotency_records_organizacion
        FOREIGN KEY (organizacion_id)
        REFERENCES organizaciones (id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    CONSTRAINT uq_idempotency_records_identidad
        UNIQUE NULLS NOT DISTINCT (organizacion_id, operacion, idempotency_key),
    CONSTRAINT ck_idempotency_records_operacion
        CHECK (
            char_length(operacion) BETWEEN 3 AND 128
            AND operacion ~ '^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)+$'
        ),
    CONSTRAINT ck_idempotency_records_key
        CHECK (
            idempotency_key = btrim(idempotency_key)
            AND idempotency_key <> ''
            AND char_length(idempotency_key) <= 256
        ),
    CONSTRAINT ck_idempotency_records_request_sha256
        CHECK (octet_length(request_sha256) = 32),
    CONSTRAINT ck_idempotency_records_resultado_tamano
        CHECK (octet_length(resultado::text) <= 32768)
);

CREATE FUNCTION proteger_historia_idempotency_records()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'agro_ops_idempotency_historia_eliminacion_prohibida';
    END IF;

    RAISE EXCEPTION USING
        ERRCODE = 'P0001',
        MESSAGE = 'agro_ops_idempotency_historia_inmutable';
END;
$$;

CREATE TRIGGER trg_idempotency_records_historia
BEFORE UPDATE OR DELETE ON idempotency_records
FOR EACH ROW
EXECUTE FUNCTION proteger_historia_idempotency_records();

CREATE FUNCTION impedir_truncate_idempotency_records()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
BEGIN
    RAISE EXCEPTION USING
        ERRCODE = 'P0001',
        MESSAGE = 'agro_ops_idempotency_historia_truncate_prohibido';
END;
$$;

CREATE TRIGGER trg_idempotency_records_truncate_prohibido
BEFORE TRUNCATE ON idempotency_records
FOR EACH STATEMENT
EXECUTE FUNCTION impedir_truncate_idempotency_records();
