CREATE OR REPLACE FUNCTION proteger_historia_usuario_rol()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'agro_ops_usuario_rol_historia_eliminacion_prohibida';
    END IF;

    IF NEW.id IS DISTINCT FROM OLD.id
        OR NEW.usuario_id IS DISTINCT FROM OLD.usuario_id
        OR NEW.rol_id IS DISTINCT FROM OLD.rol_id
        OR NEW.vigente_desde IS DISTINCT FROM OLD.vigente_desde
        OR NEW.vigente_hasta IS NULL THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'agro_ops_usuario_rol_historia_inmutable';
    END IF;

    IF OLD.vigente_hasta IS NOT NULL
        AND (
            OLD.vigente_desde > statement_timestamp()
            OR OLD.vigente_hasta <= statement_timestamp()
            OR NEW.vigente_hasta IS DISTINCT FROM statement_timestamp()
        ) THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'agro_ops_usuario_rol_historia_inmutable';
    END IF;

    RETURN NEW;
END;
$$;

CREATE OR REPLACE FUNCTION proteger_historia_rol_permiso()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'agro_ops_rol_permiso_historia_eliminacion_prohibida';
    END IF;

    IF NEW.id IS DISTINCT FROM OLD.id
        OR NEW.rol_id IS DISTINCT FROM OLD.rol_id
        OR NEW.permiso_id IS DISTINCT FROM OLD.permiso_id
        OR NEW.vigente_desde IS DISTINCT FROM OLD.vigente_desde
        OR NEW.vigente_hasta IS NULL THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'agro_ops_rol_permiso_historia_inmutable';
    END IF;

    IF OLD.vigente_hasta IS NOT NULL
        AND (
            OLD.vigente_desde > statement_timestamp()
            OR OLD.vigente_hasta <= statement_timestamp()
            OR NEW.vigente_hasta IS DISTINCT FROM statement_timestamp()
        ) THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'agro_ops_rol_permiso_historia_inmutable';
    END IF;

    RETURN NEW;
END;
$$;
