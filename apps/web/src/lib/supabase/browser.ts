import { createBrowserClient } from "@supabase/ssr";
import { createClient } from "@supabase/supabase-js";
import { supabasePublicConfig } from "./env";

export function createSupabaseBrowserClient() {
  const { url, publishableKey } = supabasePublicConfig();
  return createBrowserClient(url, publishableKey);
}

export function createSupabaseInvitationClient() {
  const { url, publishableKey } = supabasePublicConfig();

  return createClient(url, publishableKey, {
    auth: {
      autoRefreshToken: false,
      detectSessionInUrl: false,
      persistSession: false,
    },
  });
}
