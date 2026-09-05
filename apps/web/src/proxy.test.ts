import { NextRequest } from "next/server";
import { afterEach, describe, expect, it, vi } from "vitest";

const auth = vi.hoisted(() => ({
  getUser: vi.fn(),
  setCookies: vi.fn(),
}));

vi.mock("@supabase/ssr", () => ({
  createServerClient: vi.fn((_url, _key, options) => {
    auth.setCookies.mockImplementation((cookies) => options.cookies.setAll(cookies));
    return { auth: { getUser: auth.getUser } };
  }),
}));

import { proxy } from "./proxy";

describe("private route proxy", () => {
  afterEach(() => {
    vi.unstubAllEnvs();
    auth.getUser.mockReset();
    auth.setCookies.mockReset();
  });

  function request(path: string) {
    vi.stubEnv("NEXT_PUBLIC_SUPABASE_URL", "https://project.supabase.co");
    vi.stubEnv("NEXT_PUBLIC_SUPABASE_PUBLISHABLE_KEY", "sb_publishable_test");
    return new NextRequest(`http://localhost:3000${path}`);
  }

  it("redirects an anonymous deep link to login and preserves its return path", async () => {
    auth.getUser.mockResolvedValue({ data: { user: null } });

    const response = await proxy(request("/internal/system-status?tab=worker"));
    const location = new URL(response.headers.get("location")!);

    expect(response.status).toBe(307);
    expect(location.pathname).toBe("/login");
    expect(location.searchParams.get("next")).toBe(
      "/internal/system-status?tab=worker",
    );
  });

  it("lets an authenticated user reach the requested internal page", async () => {
    auth.getUser.mockResolvedValue({ data: { user: { id: "user-id" } } });

    const response = await proxy(request("/internal/system-status"));

    expect(response.status).toBe(200);
    expect(response.headers.get("location")).toBeNull();
  });

  it("preserves Supabase cookie mutations on an anonymous redirect", async () => {
    auth.getUser.mockImplementation(async () => {
      auth.setCookies([
        {
          name: "sb-auth-token",
          value: "",
          options: { maxAge: 0, path: "/" },
        },
      ]);
      return { data: { user: null } };
    });

    const response = await proxy(request("/internal"));

    expect(response.status).toBe(307);
    expect(response.headers.get("set-cookie")).toContain("sb-auth-token=");
    expect(response.headers.get("set-cookie")).toContain("Max-Age=0");
  });
});
