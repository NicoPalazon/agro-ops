import { PHASE_PRODUCTION_BUILD } from "next/constants";
import { afterEach, describe, expect, it, vi } from "vitest";
import nextConfig from "./next.config";

describe("Next.js authentication configuration", () => {
  afterEach(() => {
    vi.unstubAllEnvs();
  });

  it("rejects plaintext production Supabase and API URLs during the build phase", () => {
    vi.stubEnv("NEXT_PUBLIC_SUPABASE_URL", "http://supabase.example.test");
    vi.stubEnv("NEXT_PUBLIC_SUPABASE_PUBLISHABLE_KEY", "sb_publishable_test");
    vi.stubEnv("API_BASE_URL", "http://api.example.test");

    expect(() => nextConfig(PHASE_PRODUCTION_BUILD)).toThrow(
      "NEXT_PUBLIC_SUPABASE_URL must use HTTPS",
    );
  });
});
