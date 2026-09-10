CREATE EXTENSION IF NOT EXISTS btree_gist;

CREATE TABLE organizaciones (
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    nombre TEXT NOT NULL,
    activa BOOLEAN NOT NULL DEFAULT TRUE,
    creada_en TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT pk_organizaciones PRIMARY KEY (id),
    CONSTRAINT ck_organizaciones_nombre
        CHECK (nombre = btrim(nombre) AND nombre <> '')
);

CREATE TABLE permisos (
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    codigo TEXT NOT NULL,
    nombre TEXT NOT NULL,
    descripcion TEXT,
    activo BOOLEAN NOT NULL DEFAULT TRUE,
    creado_en TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT pk_permisos PRIMARY KEY (id),
    CONSTRAINT uq_permisos_codigo UNIQUE (codigo),
    CONSTRAINT ck_permisos_codigo
        CHECK (codigo ~ '^[a-z][a-z0-9_]*:[a-z][a-z0-9_]*$'),
    CONSTRAINT ck_permisos_nombre
        CHECK (nombre = btrim(nombre) AND nombre <> ''),
    CONSTRAINT ck_permisos_descripcion
        CHECK (descripcion IS NULL OR btrim(descripcion) <> '')
);

CREATE TABLE usuarios (
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    organizacion_id UUID NOT NULL,
    nombre_completo TEXT NOT NULL,
    activo BOOLEAN NOT NULL DEFAULT TRUE,
    creado_en TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT pk_usuarios PRIMARY KEY (id),
    CONSTRAINT fk_usuarios_organizacion
        FOREIGN KEY (organizacion_id)
        REFERENCES organizaciones (id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    CONSTRAINT ck_usuarios_nombre_completo
        CHECK (nombre_completo = btrim(nombre_completo) AND nombre_completo <> '')
);

CREATE TABLE roles (
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    organizacion_id UUID NOT NULL,
    nombre TEXT NOT NULL,
    descripcion TEXT,
    activo BOOLEAN NOT NULL DEFAULT TRUE,
    creado_en TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT pk_roles PRIMARY KEY (id),
    CONSTRAINT uq_roles_organizacion_nombre UNIQUE (organizacion_id, nombre),
    CONSTRAINT fk_roles_organizacion
        FOREIGN KEY (organizacion_id)
        REFERENCES organizaciones (id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    CONSTRAINT ck_roles_nombre
        CHECK (nombre = btrim(nombre) AND nombre <> ''),
    CONSTRAINT ck_roles_descripcion
        CHECK (descripcion IS NULL OR btrim(descripcion) <> '')
);

CREATE TABLE identidades_autenticacion_externas (
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    usuario_id UUID NOT NULL,
    proveedor TEXT NOT NULL,
    sujeto_proveedor UUID NOT NULL,
    vinculada_en TIMESTAMPTZ NOT NULL DEFAULT now(),
    desvinculada_en TIMESTAMPTZ,
    CONSTRAINT pk_identidades_autenticacion_externas PRIMARY KEY (id),
    CONSTRAINT uq_identidades_auth_proveedor_sujeto
        UNIQUE (proveedor, sujeto_proveedor),
    CONSTRAINT fk_identidades_auth_usuario
        FOREIGN KEY (usuario_id)
        REFERENCES usuarios (id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    CONSTRAINT ck_identidades_auth_proveedor
        CHECK (proveedor = btrim(proveedor) AND proveedor ~ '^[a-z][a-z0-9_]*$'),
    CONSTRAINT ck_identidades_auth_vigencia
        CHECK (desvinculada_en IS NULL OR desvinculada_en > vinculada_en)
);

ALTER TABLE identidades_autenticacion_externas
    ADD CONSTRAINT excl_identidades_auth_usuario_vigencia
    EXCLUDE USING gist (
        usuario_id WITH =,
        tstzrange(vinculada_en, desvinculada_en, '[)') WITH &&
    );

CREATE TABLE usuarios_roles (
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    usuario_id UUID NOT NULL,
    rol_id UUID NOT NULL,
    vigente_desde TIMESTAMPTZ NOT NULL DEFAULT now(),
    vigente_hasta TIMESTAMPTZ,
    CONSTRAINT pk_usuarios_roles PRIMARY KEY (id),
    CONSTRAINT fk_usuarios_roles_usuario
        FOREIGN KEY (usuario_id)
        REFERENCES usuarios (id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    CONSTRAINT fk_usuarios_roles_rol
        FOREIGN KEY (rol_id)
        REFERENCES roles (id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    CONSTRAINT ck_usuarios_roles_vigencia
        CHECK (vigente_hasta IS NULL OR vigente_hasta > vigente_desde)
);

ALTER TABLE usuarios_roles
    ADD CONSTRAINT excl_usuarios_roles_vigencia
    EXCLUDE USING gist (
        usuario_id WITH =,
        rol_id WITH =,
        tstzrange(vigente_desde, vigente_hasta, '[)') WITH &&
    );

CREATE TABLE roles_permisos (
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    rol_id UUID NOT NULL,
    permiso_id UUID NOT NULL,
    vigente_desde TIMESTAMPTZ NOT NULL DEFAULT now(),
    vigente_hasta TIMESTAMPTZ,
    CONSTRAINT pk_roles_permisos PRIMARY KEY (id),
    CONSTRAINT fk_roles_permisos_rol
        FOREIGN KEY (rol_id)
        REFERENCES roles (id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    CONSTRAINT fk_roles_permisos_permiso
        FOREIGN KEY (permiso_id)
        REFERENCES permisos (id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    CONSTRAINT ck_roles_permisos_vigencia
        CHECK (vigente_hasta IS NULL OR vigente_hasta > vigente_desde)
);

ALTER TABLE roles_permisos
    ADD CONSTRAINT excl_roles_permisos_vigencia
    EXCLUDE USING gist (
        rol_id WITH =,
        permiso_id WITH =,
        tstzrange(vigente_desde, vigente_hasta, '[)') WITH &&
    );

CREATE INDEX idx_usuarios_roles_vigentes_por_usuario
    ON usuarios_roles (usuario_id, rol_id)
    WHERE vigente_hasta IS NULL;

CREATE INDEX idx_roles_permisos_vigentes_por_rol
    ON roles_permisos (rol_id, permiso_id)
    WHERE vigente_hasta IS NULL;

CREATE FUNCTION impedir_cambio_organizacion_usuario()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.organizacion_id IS DISTINCT FROM OLD.organizacion_id THEN
        RAISE EXCEPTION 'la organizacion de un usuario es inmutable';
    END IF;

    RETURN NEW;
END;
$$;

CREATE TRIGGER trg_usuarios_organizacion_inmutable
BEFORE UPDATE OF organizacion_id ON usuarios
FOR EACH ROW
EXECUTE FUNCTION impedir_cambio_organizacion_usuario();

CREATE FUNCTION impedir_cambio_organizacion_rol()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.organizacion_id IS DISTINCT FROM OLD.organizacion_id THEN
        RAISE EXCEPTION 'la organizacion de un rol es inmutable';
    END IF;

    RETURN NEW;
END;
$$;

CREATE TRIGGER trg_roles_organizacion_inmutable
BEFORE UPDATE OF organizacion_id ON roles
FOR EACH ROW
EXECUTE FUNCTION impedir_cambio_organizacion_rol();

CREATE FUNCTION impedir_cambio_codigo_permiso()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.codigo IS DISTINCT FROM OLD.codigo THEN
        RAISE EXCEPTION 'el codigo de un permiso es inmutable';
    END IF;

    RETURN NEW;
END;
$$;

CREATE TRIGGER trg_permisos_codigo_inmutable
BEFORE UPDATE OF codigo ON permisos
FOR EACH ROW
EXECUTE FUNCTION impedir_cambio_codigo_permiso();

CREATE FUNCTION validar_usuario_rol_misma_organizacion()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog, public
AS $$
DECLARE
    organizacion_usuario UUID;
    organizacion_rol UUID;
BEGIN
    SELECT organizacion_id
    INTO organizacion_usuario
    FROM public.usuarios
    WHERE id = NEW.usuario_id;

    SELECT organizacion_id
    INTO organizacion_rol
    FROM public.roles
    WHERE id = NEW.rol_id;

    IF organizacion_usuario IS DISTINCT FROM organizacion_rol THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'agro_ops_usuario_rol_organizacion_invalida';
    END IF;

    RETURN NEW;
END;
$$;

CREATE TRIGGER trg_usuarios_roles_misma_organizacion
BEFORE INSERT OR UPDATE OF usuario_id, rol_id ON usuarios_roles
FOR EACH ROW
EXECUTE FUNCTION validar_usuario_rol_misma_organizacion();

CREATE FUNCTION proteger_historia_identidad_auth()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'agro_ops_identidad_auth_historia_eliminacion_prohibida';
    END IF;

    IF NEW.id IS DISTINCT FROM OLD.id
        OR NEW.usuario_id IS DISTINCT FROM OLD.usuario_id
        OR NEW.proveedor IS DISTINCT FROM OLD.proveedor
        OR NEW.sujeto_proveedor IS DISTINCT FROM OLD.sujeto_proveedor
        OR NEW.vinculada_en IS DISTINCT FROM OLD.vinculada_en
        OR OLD.desvinculada_en IS NOT NULL
        OR NEW.desvinculada_en IS NULL THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'agro_ops_identidad_auth_historia_inmutable';
    END IF;

    RETURN NEW;
END;
$$;

CREATE TRIGGER trg_identidades_auth_historia
BEFORE UPDATE OR DELETE ON identidades_autenticacion_externas
FOR EACH ROW
EXECUTE FUNCTION proteger_historia_identidad_auth();

CREATE FUNCTION proteger_historia_usuario_rol()
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
        OR OLD.vigente_hasta IS NOT NULL
        OR NEW.vigente_hasta IS NULL THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'agro_ops_usuario_rol_historia_inmutable';
    END IF;

    RETURN NEW;
END;
$$;

CREATE TRIGGER trg_usuarios_roles_historia
BEFORE UPDATE OR DELETE ON usuarios_roles
FOR EACH ROW
EXECUTE FUNCTION proteger_historia_usuario_rol();

CREATE FUNCTION proteger_historia_rol_permiso()
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
        OR OLD.vigente_hasta IS NOT NULL
        OR NEW.vigente_hasta IS NULL THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'agro_ops_rol_permiso_historia_inmutable';
    END IF;

    RETURN NEW;
END;
$$;

CREATE TRIGGER trg_roles_permisos_historia
BEFORE UPDATE OR DELETE ON roles_permisos
FOR EACH ROW
EXECUTE FUNCTION proteger_historia_rol_permiso();

CREATE FUNCTION impedir_truncate_historia_autorizacion()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION USING
        ERRCODE = 'P0001',
        MESSAGE = 'agro_ops_historia_autorizacion_truncate_prohibido';
END;
$$;

CREATE TRIGGER trg_identidades_auth_truncate_prohibido
BEFORE TRUNCATE ON identidades_autenticacion_externas
FOR EACH STATEMENT
EXECUTE FUNCTION impedir_truncate_historia_autorizacion();

CREATE TRIGGER trg_usuarios_roles_truncate_prohibido
BEFORE TRUNCATE ON usuarios_roles
FOR EACH STATEMENT
EXECUTE FUNCTION impedir_truncate_historia_autorizacion();

CREATE TRIGGER trg_roles_permisos_truncate_prohibido
BEFORE TRUNCATE ON roles_permisos
FOR EACH STATEMENT
EXECUTE FUNCTION impedir_truncate_historia_autorizacion();

INSERT INTO permisos (codigo, nombre, descripcion)
VALUES
    ('panel:ver', 'Ver el panel general', 'Ver el panel general.'),
    ('agricultura:ver', 'Consultar información agrícola', 'Consultar información agrícola.'),
    ('agricultura:crear', 'Crear registros agrícolas', 'Crear registros agrícolas.'),
    ('agricultura:editar', 'Modificar registros agrícolas', 'Modificar registros agrícolas.'),
    ('inventario:ver', 'Consultar inventario y existencias', 'Consultar inventario y existencias.'),
    ('inventario:ajustar', 'Registrar ajustes de inventario', 'Registrar ajustes de inventario.'),
    ('ganaderia:ver', 'Consultar información ganadera disponible en V1', 'Consultar información ganadera disponible en V1.'),
    ('ganaderia:editar', 'Modificar información ganadera habilitada en V1', 'Modificar información ganadera habilitada en V1.'),
    ('maquinaria:ver', 'Consultar maquinaria', 'Consultar maquinaria.'),
    ('maquinaria:editar', 'Modificar información de maquinaria', 'Modificar información de maquinaria.'),
    ('comercial:ver', 'Consultar información comercial', 'Consultar información comercial.'),
    ('comercial:editar', 'Modificar información comercial', 'Modificar información comercial.'),
    ('configuracion:ver', 'Consultar la configuración de Agro Ops', 'Consultar la configuración de Agro Ops.'),
    ('configuracion:administrar', 'Administrar la configuración de Agro Ops', 'Administrar la configuración de Agro Ops.'),
    ('consola_tecnica:ver', 'Consultar la consola técnica interna', 'Consultar la consola técnica interna.'),
    ('consola_tecnica:administrar', 'Administrar operaciones de la consola técnica interna', 'Administrar operaciones de la consola técnica interna.');
