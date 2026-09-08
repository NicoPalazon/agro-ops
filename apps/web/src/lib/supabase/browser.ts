import { createBrowserClient } from "@supabase/ssr";
import { supabasePublicConfig } from "./env";

export function createSupabaseBrowserClient() {
  const { url, publishableKey } = supabasePublicConfig();
  return createBrowserClient(url, publishableKey);
}
