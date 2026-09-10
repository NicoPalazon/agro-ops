CREATE TABLE jobs (
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    organizacion_id UUID,
    tipo TEXT NOT NULL,
    estado TEXT NOT NULL DEFAULT 'pendiente',
    payload JSONB NOT NULL,
    intentos INTEGER NOT NULL DEFAULT 0,
    max_intentos INTEGER NOT NULL,
    next_attempt_at TIMESTAMPTZ NOT NULL,
    bloqueado_en TIMESTAMPTZ,
    bloqueado_por UUID,
    creado_en TIMESTAMPTZ NOT NULL DEFAULT statement_timestamp(),
    actualizado_en TIMESTAMPTZ NOT NULL DEFAULT statement_timestamp(),
    completado_en TIMESTAMPTZ,
    ultimo_error TEXT,
    CONSTRAINT pk_jobs PRIMARY KEY (id),
    CONSTRAINT fk_jobs_organizacion
        FOREIGN KEY (organizacion_id)
        REFERENCES organizaciones (id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    CONSTRAINT ck_jobs_tipo
        CHECK (
            char_length(tipo) BETWEEN 3 AND 128
            AND tipo ~ '^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)+$'
        ),
    CONSTRAINT ck_jobs_estado
        CHECK (estado IN ('pendiente', 'ejecutando', 'completado', 'agotado')),
    CONSTRAINT ck_jobs_intentos
        CHECK (intentos >= 0 AND max_intentos > 0 AND intentos <= max_intentos),
    CONSTRAINT ck_jobs_ultimo_error
        CHECK (
            ultimo_error IS NULL
            OR (
                ultimo_error = btrim(ultimo_error)
                AND ultimo_error <> ''
                AND char_length(ultimo_error) <= 1024
            )
        ),
    CONSTRAINT ck_jobs_estado_bloqueo
        CHECK (
            (
                estado = 'pendiente'
                AND bloqueado_en IS NULL
                AND bloqueado_por IS NULL
                AND completado_en IS NULL
            )
            OR (
                estado = 'ejecutando'
                AND bloqueado_en IS NOT NULL
                AND bloqueado_por IS NOT NULL
                AND completado_en IS NULL
            )
            OR (
                estado = 'completado'
                AND bloqueado_en IS NULL
                AND bloqueado_por IS NULL
                AND completado_en IS NOT NULL
            )
            OR (
                estado = 'agotado'
                AND bloqueado_en IS NULL
                AND bloqueado_por IS NULL
                AND completado_en IS NULL
            )
        )
);

CREATE INDEX idx_jobs_pendientes_claim
    ON jobs (next_attempt_at, creado_en, id)
    WHERE estado = 'pendiente';

CREATE INDEX idx_jobs_ejecutando_recuperacion
    ON jobs (bloqueado_en, id)
    WHERE estado = 'ejecutando';

CREATE INDEX idx_jobs_diagnostico_estado_actualizacion
    ON jobs (estado, actualizado_en DESC, id DESC);

CREATE FUNCTION proteger_transiciones_jobs()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
BEGIN
    IF NEW.id IS DISTINCT FROM OLD.id
        OR NEW.organizacion_id IS DISTINCT FROM OLD.organizacion_id
        OR NEW.tipo IS DISTINCT FROM OLD.tipo
        OR NEW.payload IS DISTINCT FROM OLD.payload
        OR NEW.max_intentos IS DISTINCT FROM OLD.max_intentos
        OR NEW.creado_en IS DISTINCT FROM OLD.creado_en THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'agro_ops_job_datos_inmutables';
    END IF;

    IF OLD.estado = 'pendiente'
        AND NEW.estado = 'ejecutando'
        AND NEW.intentos = OLD.intentos + 1
        AND OLD.intentos < OLD.max_intentos THEN
        RETURN NEW;
    END IF;

    IF OLD.estado = 'ejecutando'
        AND NEW.estado = 'completado'
        AND NEW.intentos = OLD.intentos THEN
        RETURN NEW;
    END IF;

    IF OLD.estado = 'ejecutando'
        AND NEW.estado = 'pendiente'
        AND NEW.intentos = OLD.intentos
        AND NEW.intentos < NEW.max_intentos THEN
        RETURN NEW;
    END IF;

    IF OLD.estado = 'ejecutando'
        AND NEW.estado = 'agotado'
        AND NEW.intentos = OLD.intentos
        AND NEW.intentos >= NEW.max_intentos THEN
        RETURN NEW;
    END IF;

    RAISE EXCEPTION USING
        ERRCODE = 'P0001',
        MESSAGE = 'agro_ops_job_transicion_invalida';
END;
$$;

CREATE TRIGGER trg_jobs_transiciones
BEFORE UPDATE ON jobs
FOR EACH ROW
EXECUTE FUNCTION proteger_transiciones_jobs();
