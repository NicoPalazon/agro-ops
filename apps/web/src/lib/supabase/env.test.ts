import { afterEach, describe, expect, it, vi } from "vitest";
import { apiBaseUrl, supabasePublicConfig } from "./env";

describe("web runtime authentication configuration", () => {
  afterEach(() => {
    vi.unstubAllEnvs();
  });

  it("allows local HTTP endpoints", () => {
    vi.stubEnv("NODE_ENV", "development");
    vi.stubEnv("NEXT_PUBLIC_SUPABASE_URL", "http://127.0.0.1:54321");
    vi.stubEnv("NEXT_PUBLIC_SUPABASE_PUBLISHABLE_KEY", "sb_publishable_test");
    vi.stubEnv("API_BASE_URL", "http://api:8080");

    expect(supabasePublicConfig().url).toBe("http://127.0.0.1:54321");
    expect(apiBaseUrl()).toBe("http://api:8080");
  });

  it("rejects plaintext Supabase and API URLs in production", () => {
    vi.stubEnv("NODE_ENV", "production");
    vi.stubEnv("NEXT_PUBLIC_SUPABASE_URL", "http://project.supabase.co");
    vi.stubEnv("NEXT_PUBLIC_SUPABASE_PUBLISHABLE_KEY", "sb_publishable_test");
    vi.stubEnv("API_BASE_URL", "http://api.example");

    expect(supabasePublicConfig).toThrow(/NEXT_PUBLIC_SUPABASE_URL must use HTTPS/);
    expect(apiBaseUrl).toThrow(/API_BASE_URL must use HTTPS/);
  });

  it("requires API_BASE_URL in production", () => {
    vi.stubEnv("NODE_ENV", "production");
    vi.stubEnv("NEXT_PUBLIC_SUPABASE_URL", "https://project.supabase.co");
    vi.stubEnv("NEXT_PUBLIC_SUPABASE_PUBLISHABLE_KEY", "sb_publishable_test");
    vi.stubEnv("API_BASE_URL", "");

    expect(apiBaseUrl).toThrow(/API_BASE_URL must be configured/);
  });
});
