import { createSupabaseServerClient } from "@/lib/supabase/server";

/** Returns the current session access token for an already-protected request. */
export async function authenticatedAccessToken(): Promise<string | null> {
  const supabase = await createSupabaseServerClient();
  const {
    data: { session },
  } = await supabase.auth.getSession();

  return session?.access_token ?? null;
}
