ALTER TABLE fuentes_geograficas
    ADD CONSTRAINT uq_fuentes_geograficas_id_organizacion_establecimiento
    UNIQUE (id, organizacion_id, establecimiento_id);

CREATE TABLE fuentes_geograficas_contribuciones_canonicas (
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    organizacion_id UUID NOT NULL,
    establecimiento_id UUID NOT NULL,
    fuente_geografica_id UUID NOT NULL,
    confirmado_por UUID NOT NULL,
    confirmado_en TIMESTAMPTZ NOT NULL DEFAULT statement_timestamp(),
    CONSTRAINT pk_fuentes_geograficas_contribuciones_canonicas PRIMARY KEY (id),
    CONSTRAINT uq_fuentes_geograficas_contribuciones_fuente
        UNIQUE (fuente_geografica_id),
    CONSTRAINT fk_fuentes_geograficas_contribuciones_organizacion
        FOREIGN KEY (organizacion_id)
        REFERENCES organizaciones (id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    CONSTRAINT fk_fuentes_geograficas_contribuciones_establecimiento
        FOREIGN KEY (establecimiento_id, organizacion_id)
        REFERENCES establecimientos (id, organizacion_id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    CONSTRAINT fk_fuentes_geograficas_contribuciones_fuente
        FOREIGN KEY (fuente_geografica_id, organizacion_id, establecimiento_id)
        REFERENCES fuentes_geograficas (id, organizacion_id, establecimiento_id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    CONSTRAINT fk_fuentes_geograficas_contribuciones_confirmado_por
        FOREIGN KEY (confirmado_por)
        REFERENCES usuarios (id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT
);

CREATE INDEX idx_fuentes_geograficas_contribuciones_establecimiento
    ON fuentes_geograficas_contribuciones_canonicas
    (organizacion_id, establecimiento_id, fuente_geografica_id);

CREATE FUNCTION validar_confirmador_fuente_geografica_misma_organizacion()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
DECLARE
    organizacion_confirmador UUID;
BEGIN
    SELECT organizacion_id
    INTO organizacion_confirmador
    FROM public.usuarios
    WHERE id = NEW.confirmado_por;

    IF organizacion_confirmador IS DISTINCT FROM NEW.organizacion_id THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'agro_ops_fuente_geografica_confirmador_organizacion_invalida';
    END IF;

    RETURN NEW;
END;
$$;

CREATE TRIGGER trg_fuentes_geograficas_contribuciones_autor_misma_organizacion
BEFORE INSERT OR UPDATE OF organizacion_id, confirmado_por
ON fuentes_geograficas_contribuciones_canonicas
FOR EACH ROW
EXECUTE FUNCTION validar_confirmador_fuente_geografica_misma_organizacion();
