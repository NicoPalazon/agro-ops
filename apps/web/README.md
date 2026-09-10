# Agro Ops Web

Aplicación Next.js de Agro Ops. La ruta inicial deriva al flujo privado y las
rutas `/internal/*` y `/configuracion/*` requieren una sesión de Supabase Auth.
Los layouts protegidos consultan `/me` en el backend antes de renderizar; ocultar
una opción en React es sólo una ayuda de UX y no reemplaza la autorización de
Rust.

## Configuración

Variables requeridas:

- `NEXT_PUBLIC_SUPABASE_URL`
- `NEXT_PUBLIC_SUPABASE_PUBLISHABLE_KEY`
- `API_BASE_URL` (sólo servidor)

Nunca configure una clave secreta de Supabase como `NEXT_PUBLIC_*` ni en Vercel.
Después del bootstrap del primer administrador, la administración cotidiana se
realiza en `/configuracion/usuarios` y `/configuracion/roles`.

## Desarrollo y verificación

Desde este directorio:

```bash
pnpm install --frozen-lockfile
pnpm dev
pnpm lint
pnpm exec next typegen
pnpm exec tsc --noEmit
pnpm test
pnpm build
```

## Deploy en Vercel

Configure `apps/web` como **Root Directory** y use el build predeterminado
`pnpm build`. En staging y producción, las URLs configuradas deben usar HTTPS.
