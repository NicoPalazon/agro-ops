ALTER TABLE fuentes_geograficas
    DROP CONSTRAINT ck_fuentes_geograficas_tipo_origen,
    DROP CONSTRAINT ck_fuentes_geograficas_version_parser,
    DROP CONSTRAINT ck_fuentes_geograficas_texto_fuente_original;

ALTER TABLE fuentes_geograficas
    ALTER COLUMN external_reference_id DROP NOT NULL,
    ADD CONSTRAINT ck_fuentes_geograficas_tipo_origen
        CHECK (tipo_origen IN ('senasa_renspa', 'manual', 'importada')),
    ADD CONSTRAINT ck_fuentes_geograficas_texto_fuente_original
        CHECK (texto_fuente_original <> '' AND octet_length(texto_fuente_original) <= 1000000),
    ADD CONSTRAINT ck_fuentes_geograficas_version_parser
        CHECK (
            (tipo_origen = 'senasa_renspa' AND version_parser = 'senasa_pasted_polygon_v1')
            OR
            (tipo_origen IN ('manual', 'importada') AND version_parser = 'geojson_polygon_multipolygon_v1')
        ),
    ADD CONSTRAINT ck_fuentes_geograficas_referencia_segun_origen
        CHECK (
            (tipo_origen = 'senasa_renspa' AND external_reference_id IS NOT NULL)
            OR
            (tipo_origen IN ('manual', 'importada') AND external_reference_id IS NULL)
        );

CREATE OR REPLACE FUNCTION validar_fuente_geografica_external_reference()
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
    IF NEW.tipo_origen IN ('manual', 'importada') THEN
        IF NEW.external_reference_id IS NOT NULL THEN
            RAISE EXCEPTION USING ERRCODE = 'P0001', MESSAGE = 'agro_ops_fuente_geografica_external_reference_invalida';
        END IF;
        RETURN NEW;
    END IF;

    SELECT organizacion_id, sistema_externo, entidad_tipo, entidad_id
    INTO referencia_organizacion, referencia_sistema, referencia_entidad_tipo, referencia_entidad_id
    FROM public.external_references
    WHERE id = NEW.external_reference_id;

    IF NEW.tipo_origen <> 'senasa_renspa'
        OR referencia_organizacion IS DISTINCT FROM NEW.organizacion_id
        OR referencia_sistema IS DISTINCT FROM 'senasa'
        OR referencia_entidad_tipo IS DISTINCT FROM 'establecimiento'
        OR referencia_entidad_id IS DISTINCT FROM NEW.establecimiento_id THEN
        RAISE EXCEPTION USING ERRCODE = 'P0001', MESSAGE = 'agro_ops_fuente_geografica_external_reference_invalida';
    END IF;
    RETURN NEW;
END;
$$;

DROP TRIGGER trg_fuentes_geograficas_external_reference_valida ON fuentes_geograficas;
CREATE TRIGGER trg_fuentes_geograficas_external_reference_valida
BEFORE INSERT OR UPDATE OF organizacion_id, establecimiento_id, external_reference_id, tipo_origen ON fuentes_geograficas
FOR EACH ROW EXECUTE FUNCTION validar_fuente_geografica_external_reference();
