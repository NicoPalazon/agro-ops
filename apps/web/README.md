This is a [Next.js](https://nextjs.org) project bootstrapped with [`create-next-app`](https://nextjs.org/docs/app/api-reference/cli/create-next-app).

## Authentication configuration

The Internal Console uses Supabase Auth with email/password accounts only. Set
`NEXT_PUBLIC_SUPABASE_URL`, `NEXT_PUBLIC_SUPABASE_PUBLISHABLE_KEY`, and
`API_BASE_URL` before running or building the web application. The public values
must be the URL and publishable key for the matching Supabase project; never use
a secret key here. The application intentionally provides no public sign-up
screen or local authentication bypass. After the first administrator bootstrap,
routine invitations and Agro Ops access are managed at `/configuracion/usuarios`.

Unauthenticated requests to `/internal/*` and `/configuracion/*` redirect to
`/login`. Supabase SSR cookies persist the authenticated session. Server layouts
call the backend `/me` endpoint before rendering protected content, and server
actions forward the current access token to protected administration endpoints.

The backend OpenAPI document at `/openapi.json` is also private. Retrieve it with
an enabled user's Supabase access token as `Authorization: Bearer <token>`; do not
use a service-role key.

## Getting Started

First, run the development server:

```bash
npm run dev
# or
yarn dev
# or
pnpm dev
# or
bun dev
```

Open [http://localhost:3000](http://localhost:3000) with your browser to see the result.

You can start editing the page by modifying `app/page.tsx`. The page auto-updates as you edit the file.

This project uses [`next/font`](https://nextjs.org/docs/app/building-your-application/optimizing/fonts) to automatically optimize and load [Geist](https://vercel.com/font), a new font family for Vercel.

## Learn More

To learn more about Next.js, take a look at the following resources:

- [Next.js Documentation](https://nextjs.org/docs) - learn about Next.js features and API.
- [Learn Next.js](https://nextjs.org/learn) - an interactive Next.js tutorial.

You can check out [the Next.js GitHub repository](https://github.com/vercel/next.js) - your feedback and contributions are welcome!

## Deploy on Vercel

Create the Vercel project from this repository with **Root Directory** set to
`apps/web`. Vercel detects the Next.js framework and the app's local pnpm lockfile;
leave the build command at its default (`pnpm build`).

For the `feat/walking-skeleton` Preview deployment, configure these environment
variables with the **Preview** target:

| Name | Value |
| --- | --- |
| `NEXT_PUBLIC_SUPABASE_URL` | URL of the existing staging Supabase project |
| `NEXT_PUBLIC_SUPABASE_PUBLISHABLE_KEY` | Publishable/anon key of that staging Supabase project |
| `API_BASE_URL` | `https://agro-ops-staging.up.railway.app` |

`API_BASE_URL` is deliberately server-only: it is used by server-rendered
backend status requests and must not be prefixed with `NEXT_PUBLIC_`. Do not add
a Supabase secret key, or any other privileged credential, to Vercel.

The application validates these values during production builds, requires HTTPS
outside local development, and fails the build if any required value is missing.
