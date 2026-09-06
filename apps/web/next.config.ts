import type { NextConfig } from "next";
import {
  PHASE_DEVELOPMENT_SERVER,
  PHASE_PRODUCTION_BUILD,
  PHASE_PRODUCTION_SERVER,
} from "next/constants";

function configuredUrl(name: string, value: string, requireHttps: boolean) {
  let url: URL;

  try {
    url = new URL(value);
  } catch {
    throw new Error(`${name} must be an absolute HTTP(S) URL.`);
  }

  if (url.protocol !== "http:" && url.protocol !== "https:") {
    throw new Error(`${name} must be an absolute HTTP(S) URL.`);
  }

  if (requireHttps && url.protocol !== "https:") {
    throw new Error(`${name} must use HTTPS outside local development.`);
  }
}

export default function nextConfig(phase: string): NextConfig {
  const isProduction =
    phase === PHASE_PRODUCTION_BUILD || phase === PHASE_PRODUCTION_SERVER;
  const requiresAuthConfiguration = isProduction || phase === PHASE_DEVELOPMENT_SERVER;

  if (requiresAuthConfiguration) {
    const missing = [
      "NEXT_PUBLIC_SUPABASE_URL",
      "NEXT_PUBLIC_SUPABASE_PUBLISHABLE_KEY",
      ...(isProduction ? ["API_BASE_URL"] : []),
    ].filter((name) => !process.env[name]);

    if (missing.length > 0) {
      throw new Error(
        `Authentication build configuration is missing: ${missing.join(", ")}`,
      );
    }

    configuredUrl(
      "NEXT_PUBLIC_SUPABASE_URL",
      process.env.NEXT_PUBLIC_SUPABASE_URL!,
      isProduction,
    );
    if (isProduction) {
      configuredUrl("API_BASE_URL", process.env.API_BASE_URL!, true);
    }
  }

  return {};
}
