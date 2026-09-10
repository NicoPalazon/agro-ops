CREATE TABLE audit_events (
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    organizacion_id UUID NOT NULL,
    actor_tipo TEXT NOT NULL,
    actor_usuario_id UUID,
    accion TEXT NOT NULL,
    entidad_tipo TEXT NOT NULL,
    entidad_id UUID,
    referencia TEXT,
    estado_anterior JSONB,
    estado_posterior JSONB,
    ocurrido_en TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT pk_audit_events PRIMARY KEY (id),
    CONSTRAINT fk_audit_events_organizacion
        FOREIGN KEY (organizacion_id)
        REFERENCES organizaciones (id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    CONSTRAINT fk_audit_events_actor_usuario
        FOREIGN KEY (actor_usuario_id)
        REFERENCES usuarios (id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    CONSTRAINT ck_audit_events_actor
        CHECK (
            (actor_tipo = 'usuario' AND actor_usuario_id IS NOT NULL)
            OR (actor_tipo = 'sistema' AND actor_usuario_id IS NULL)
        ),
    CONSTRAINT ck_audit_events_accion
        CHECK (accion ~ '^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)+$'),
    CONSTRAINT ck_audit_events_entidad_tipo
        CHECK (entidad_tipo ~ '^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)*$'),
    CONSTRAINT ck_audit_events_referencia
        CHECK (referencia IS NULL OR (referencia = btrim(referencia) AND referencia <> ''))
);

CREATE INDEX idx_audit_events_organizacion_ocurrido_en
    ON audit_events (organizacion_id, ocurrido_en DESC, id DESC);

CREATE INDEX idx_audit_events_entidad
    ON audit_events (organizacion_id, entidad_tipo, entidad_id, ocurrido_en DESC)
    WHERE entidad_id IS NOT NULL;

CREATE FUNCTION validar_actor_audit_misma_organizacion()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
DECLARE
    organizacion_actor UUID;
BEGIN
    IF NEW.actor_tipo = 'usuario' THEN
        SELECT organizacion_id
        INTO organizacion_actor
        FROM public.usuarios
        WHERE id = NEW.actor_usuario_id;

        IF organizacion_actor IS DISTINCT FROM NEW.organizacion_id THEN
            RAISE EXCEPTION USING
                ERRCODE = 'P0001',
                MESSAGE = 'agro_ops_audit_actor_organizacion_invalida';
        END IF;
    END IF;

    RETURN NEW;
END;
$$;

CREATE TRIGGER trg_audit_events_actor_misma_organizacion
BEFORE INSERT OR UPDATE OF organizacion_id, actor_tipo, actor_usuario_id ON audit_events
FOR EACH ROW
WHEN (NEW.actor_tipo = 'usuario' AND NEW.actor_usuario_id IS NOT NULL)
EXECUTE FUNCTION validar_actor_audit_misma_organizacion();

CREATE FUNCTION proteger_historia_audit()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'agro_ops_audit_historia_eliminacion_prohibida';
    END IF;

    RAISE EXCEPTION USING
        ERRCODE = 'P0001',
        MESSAGE = 'agro_ops_audit_historia_inmutable';
END;
$$;

CREATE TRIGGER trg_audit_events_historia
BEFORE UPDATE OR DELETE ON audit_events
FOR EACH ROW
EXECUTE FUNCTION proteger_historia_audit();

CREATE FUNCTION impedir_truncate_historia_audit()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION USING
        ERRCODE = 'P0001',
        MESSAGE = 'agro_ops_audit_historia_truncate_prohibido';
END;
$$;

CREATE TRIGGER trg_audit_events_truncate_prohibido
BEFORE TRUNCATE ON audit_events
FOR EACH STATEMENT
EXECUTE FUNCTION impedir_truncate_historia_audit();

ALTER FUNCTION public.proteger_historia_audit()
    SET search_path = pg_catalog;

ALTER FUNCTION public.impedir_truncate_historia_audit()
    SET search_path = pg_catalog;
