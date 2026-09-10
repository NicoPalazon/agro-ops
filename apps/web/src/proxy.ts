import { createServerClient } from "@supabase/ssr";
import { NextResponse, type NextRequest } from "next/server";
import { safeReturnPath } from "@/lib/auth/return-path";
import { supabasePublicConfig } from "@/lib/supabase/env";

export async function proxy(request: NextRequest) {
  let response = NextResponse.next({ request });
  const { url, publishableKey } = supabasePublicConfig();
  const supabase = createServerClient(url, publishableKey, {
    cookies: {
      getAll() {
        return request.cookies.getAll();
      },
      setAll(cookiesToSet) {
        for (const { name, value } of cookiesToSet) {
          request.cookies.set(name, value);
        }
        response = NextResponse.next({ request });
        for (const { name, value, options } of cookiesToSet) {
          response.cookies.set(name, value, options);
        }
      },
    },
  });
  // getClaims cryptographically verifies the access token against Supabase's
  // signing keys while this SSR client persists any refreshed cookies. The
  // authenticated request only needs claims here, not a user-profile lookup.
  const {
    data: claims,
  } = await supabase.auth.getClaims();

  const protectedRoute =
    request.nextUrl.pathname.startsWith("/internal") ||
    request.nextUrl.pathname.startsWith("/configuracion");

  if (protectedRoute && !claims) {
    const loginUrl = request.nextUrl.clone();
    loginUrl.pathname = "/login";
    loginUrl.search = "";
    loginUrl.searchParams.set(
      "next",
      safeReturnPath(`${request.nextUrl.pathname}${request.nextUrl.search}`),
    );
    const redirectResponse = NextResponse.redirect(loginUrl);
    for (const cookie of response.cookies.getAll()) {
      redirectResponse.cookies.set(cookie);
    }
    return redirectResponse;
  }

  return response;
}

export const config = {
  matcher: ["/internal/:path*", "/configuracion/:path*"],
};
