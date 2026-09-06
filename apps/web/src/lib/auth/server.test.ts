import { afterEach, describe, expect, it, vi } from "vitest";

const auth = vi.hoisted(() => ({
  getSession: vi.fn(),
  getUser: vi.fn(),
}));

vi.mock("@/lib/supabase/server", () => ({
  createSupabaseServerClient: vi.fn(async () => ({ auth })),
}));

import { authenticatedAccessToken } from "./server";

describe("authenticatedAccessToken", () => {
  afterEach(() => {
    auth.getSession.mockReset();
    auth.getUser.mockReset();
  });

  it("reads the existing session token without another user verification", async () => {
    auth.getSession.mockResolvedValue({
      data: { session: { access_token: "access-token" } },
    });

    await expect(authenticatedAccessToken()).resolves.toBe("access-token");
    expect(auth.getSession).toHaveBeenCalledOnce();
    expect(auth.getUser).not.toHaveBeenCalled();
  });

  it("returns null when no session token is available", async () => {
    auth.getSession.mockResolvedValue({ data: { session: null } });

    await expect(authenticatedAccessToken()).resolves.toBeNull();
  });
});
