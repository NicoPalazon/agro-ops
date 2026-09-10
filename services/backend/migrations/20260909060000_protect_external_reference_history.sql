CREATE OR REPLACE FUNCTION proteger_identidad_external_reference()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'agro_ops_external_reference_eliminacion_prohibida';
    END IF;

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

DROP TRIGGER trg_external_references_identidad_inmutable ON external_references;

CREATE TRIGGER trg_external_references_identidad_inmutable
BEFORE UPDATE OR DELETE ON external_references
FOR EACH ROW
EXECUTE FUNCTION proteger_identidad_external_reference();

CREATE FUNCTION impedir_truncate_external_references()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
BEGIN
    RAISE EXCEPTION USING
        ERRCODE = 'P0001',
        MESSAGE = 'agro_ops_external_reference_truncate_prohibido';
END;
$$;

CREATE TRIGGER trg_external_references_truncate_prohibido
BEFORE TRUNCATE ON external_references
FOR EACH STATEMENT
EXECUTE FUNCTION impedir_truncate_external_references();
