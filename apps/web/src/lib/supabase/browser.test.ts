import { beforeEach, describe, expect, it, vi } from "vitest";

const { createBrowserClient, createClient } = vi.hoisted(() => ({
  createBrowserClient: vi.fn(() => ({ kind: "browser" })),
  createClient: vi.fn(() => ({ kind: "invitation" })),
}));

vi.mock("@supabase/ssr", () => ({ createBrowserClient }));
vi.mock("@supabase/supabase-js", () => ({ createClient }));
vi.mock("./env", () => ({
  supabasePublicConfig: vi.fn(() => ({
    url: "https://project.supabase.co",
    publishableKey: "publishable-key",
  })),
}));

import {
  createSupabaseBrowserClient,
  createSupabaseInvitationClient,
} from "./browser";

describe("clientes Supabase del navegador", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("conserva el cliente SSR normal para las sesiones de la aplicación", () => {
    expect(createSupabaseBrowserClient()).toEqual({ kind: "browser" });
    expect(createBrowserClient).toHaveBeenCalledWith(
      "https://project.supabase.co",
      "publishable-key",
    );
  });

  it("aísla la invitación en una sesión sin persistencia ni detección automática", () => {
    expect(createSupabaseInvitationClient()).toEqual({ kind: "invitation" });
    expect(createClient).toHaveBeenCalledWith(
      "https://project.supabase.co",
      "publishable-key",
      {
        auth: {
          autoRefreshToken: false,
          detectSessionInUrl: false,
          persistSession: false,
        },
      },
    );
  });
});
