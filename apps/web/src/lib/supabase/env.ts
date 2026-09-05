export interface SupabasePublicConfig {
  url: string;
  publishableKey: string;
}

function requiresHttps(): boolean {
  return process.env.NODE_ENV === "production";
}

function configuredHttpUrl(name: string, value: string): string {
  let url: URL;

  try {
    url = new URL(value);
  } catch {
    throw new Error(`${name} must be an absolute HTTP(S) URL.`);
  }

  if (url.protocol !== "http:" && url.protocol !== "https:") {
    throw new Error(`${name} must be an absolute HTTP(S) URL.`);
  }

  if (requiresHttps() && url.protocol !== "https:") {
    throw new Error(`${name} must use HTTPS outside local development.`);
  }

  return value;
}

export function supabasePublicConfig(): SupabasePublicConfig {
  const url = process.env.NEXT_PUBLIC_SUPABASE_URL;
  const publishableKey = process.env.NEXT_PUBLIC_SUPABASE_PUBLISHABLE_KEY;

  if (!url || !publishableKey) {
    throw new Error(
      "NEXT_PUBLIC_SUPABASE_URL and NEXT_PUBLIC_SUPABASE_PUBLISHABLE_KEY must be configured for authentication.",
    );
  }

  return { url: configuredHttpUrl("NEXT_PUBLIC_SUPABASE_URL", url), publishableKey };
}

export function apiBaseUrl(): string | null {
  const value = process.env.API_BASE_URL;

  if (!value) {
    if (requiresHttps()) {
      throw new Error("API_BASE_URL must be configured outside local development.");
    }
    return null;
  }

  return configuredHttpUrl("API_BASE_URL", value);
}
