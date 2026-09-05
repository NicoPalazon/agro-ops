import { createSupabaseServerClient } from "@/lib/supabase/server";

/** Returns a verified user's current access token for backend requests. */
export async function authenticatedAccessToken(): Promise<string | null> {
  const supabase = await createSupabaseServerClient();
  const [{ data: userResult }, { data: sessionResult }] = await Promise.all([
    supabase.auth.getUser(),
    supabase.auth.getSession(),
  ]);

  return userResult.user && sessionResult.session?.access_token
    ? sessionResult.session.access_token
    : null;
}
