-- These trigger functions resolve only PostgreSQL built-ins and trigger context.
-- Keep their execution namespace fixed to prevent caller-controlled path lookup.
ALTER FUNCTION public.impedir_cambio_organizacion_usuario()
    SET search_path = pg_catalog;

ALTER FUNCTION public.impedir_cambio_organizacion_rol()
    SET search_path = pg_catalog;

ALTER FUNCTION public.impedir_cambio_codigo_permiso()
    SET search_path = pg_catalog;

ALTER FUNCTION public.proteger_historia_identidad_auth()
    SET search_path = pg_catalog;

ALTER FUNCTION public.proteger_historia_usuario_rol()
    SET search_path = pg_catalog;

ALTER FUNCTION public.proteger_historia_rol_permiso()
    SET search_path = pg_catalog;

ALTER FUNCTION public.impedir_truncate_historia_autorizacion()
    SET search_path = pg_catalog;
