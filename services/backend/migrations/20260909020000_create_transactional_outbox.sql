CREATE TABLE outbox_events (
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    organizacion_id UUID,
    destino TEXT NOT NULL,
    evento_tipo TEXT NOT NULL,
    entidad_tipo TEXT,
    entidad_id UUID,
    referencia TEXT,
    idempotency_key TEXT NOT NULL,
    payload JSONB NOT NULL,
    ocurrido_en TIMESTAMPTZ NOT NULL DEFAULT statement_timestamp(),
    creado_en TIMESTAMPTZ NOT NULL DEFAULT statement_timestamp(),
    CONSTRAINT pk_outbox_events PRIMARY KEY (id),
    CONSTRAINT fk_outbox_events_organizacion
        FOREIGN KEY (organizacion_id)
        REFERENCES organizaciones (id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    CONSTRAINT uq_outbox_events_idempotencia
        UNIQUE NULLS NOT DISTINCT (organizacion_id, destino, idempotency_key),
    CONSTRAINT ck_outbox_events_destino
        CHECK (
            char_length(destino) BETWEEN 1 AND 64
            AND destino ~ '^[a-z][a-z0-9_]*$'
        ),
    CONSTRAINT ck_outbox_events_evento_tipo
        CHECK (
            char_length(evento_tipo) BETWEEN 3 AND 128
            AND evento_tipo ~ '^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)+$'
        ),
    CONSTRAINT ck_outbox_events_entidad_tipo
        CHECK (
            entidad_tipo IS NULL
            OR (
                char_length(entidad_tipo) BETWEEN 1 AND 128
                AND entidad_tipo ~ '^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)*$'
            )
        ),
    CONSTRAINT ck_outbox_events_referencia
        CHECK (
            referencia IS NULL
            OR (
                referencia = btrim(referencia)
                AND referencia <> ''
                AND char_length(referencia) <= 256
            )
        ),
    CONSTRAINT ck_outbox_events_idempotency_key
        CHECK (
            idempotency_key = btrim(idempotency_key)
            AND idempotency_key <> ''
            AND char_length(idempotency_key) <= 256
        )
);

CREATE INDEX idx_outbox_events_diagnostico
    ON outbox_events (destino, evento_tipo, creado_en DESC, id DESC);

CREATE TABLE outbox_job_links (
    outbox_event_id UUID NOT NULL,
    job_id UUID NOT NULL,
    creado_en TIMESTAMPTZ NOT NULL DEFAULT statement_timestamp(),
    CONSTRAINT pk_outbox_job_links PRIMARY KEY (outbox_event_id),
    CONSTRAINT uq_outbox_job_links_job UNIQUE (job_id),
    CONSTRAINT fk_outbox_job_links_evento
        FOREIGN KEY (outbox_event_id)
        REFERENCES outbox_events (id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    CONSTRAINT fk_outbox_job_links_job
        FOREIGN KEY (job_id)
        REFERENCES jobs (id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT
);

CREATE FUNCTION proteger_historia_outbox()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'agro_ops_outbox_historia_eliminacion_prohibida';
    END IF;

    RAISE EXCEPTION USING
        ERRCODE = 'P0001',
        MESSAGE = 'agro_ops_outbox_historia_inmutable';
END;
$$;

CREATE TRIGGER trg_outbox_events_historia
BEFORE UPDATE OR DELETE ON outbox_events
FOR EACH ROW
EXECUTE FUNCTION proteger_historia_outbox();

CREATE TRIGGER trg_outbox_job_links_historia
BEFORE UPDATE OR DELETE ON outbox_job_links
FOR EACH ROW
EXECUTE FUNCTION proteger_historia_outbox();

CREATE FUNCTION impedir_truncate_historia_outbox()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
BEGIN
    RAISE EXCEPTION USING
        ERRCODE = 'P0001',
        MESSAGE = 'agro_ops_outbox_historia_truncate_prohibido';
END;
$$;

CREATE TRIGGER trg_outbox_events_truncate_prohibido
BEFORE TRUNCATE ON outbox_events
FOR EACH STATEMENT
EXECUTE FUNCTION impedir_truncate_historia_outbox();

CREATE TRIGGER trg_outbox_job_links_truncate_prohibido
BEFORE TRUNCATE ON outbox_job_links
FOR EACH STATEMENT
EXECUTE FUNCTION impedir_truncate_historia_outbox();
