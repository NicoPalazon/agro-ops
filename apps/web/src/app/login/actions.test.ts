import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const navigation = vi.hoisted(() => ({ redirect: vi.fn() }));
const supabase = vi.hoisted(() => ({
  signInWithPassword: vi.fn(),
  signOut: vi.fn(),
}));

vi.mock("next/navigation", () => navigation);
vi.mock("@/lib/supabase/server", () => ({
  createSupabaseServerClient: vi.fn(async () => ({ auth: supabase })),
}));

import { login, logout } from "./actions";

const redirected = new Error("redirected");

describe("login actions", () => {
  beforeEach(() => {
    navigation.redirect.mockImplementation(() => {
      throw redirected;
    });
  });

  afterEach(() => {
    navigation.redirect.mockReset();
    supabase.signInWithPassword.mockReset();
    supabase.signOut.mockReset();
  });

  it("retains credential sign-in and safe return redirects", async () => {
    supabase.signInWithPassword.mockResolvedValue({ error: null });
    const formData = new FormData();
    formData.set("email", "operator@example.com");
    formData.set("password", "password");
    formData.set("next", "/internal/system-status");

    await expect(login(formData)).rejects.toBe(redirected);
    expect(supabase.signInWithPassword).toHaveBeenCalledWith({
      email: "operator@example.com",
      password: "password",
    });
    expect(navigation.redirect).toHaveBeenCalledWith("/internal/system-status");
  });

  it("retains server-side sign-out and login redirect", async () => {
    supabase.signOut.mockResolvedValue({ error: null });

    await expect(logout()).rejects.toBe(redirected);
    expect(supabase.signOut).toHaveBeenCalledOnce();
    expect(navigation.redirect).toHaveBeenCalledWith("/login");
  });
});
