import { NextRequest } from "next/server";
import { afterEach, describe, expect, it, vi } from "vitest";

const auth = vi.hoisted(() => ({
  getClaims: vi.fn(),
  setCookies: vi.fn(),
}));

vi.mock("@supabase/ssr", () => ({
  createServerClient: vi.fn((_url, _key, options) => {
    auth.setCookies.mockImplementation((cookies) => options.cookies.setAll(cookies));
    return { auth: { getClaims: auth.getClaims } };
  }),
}));

import { proxy } from "./proxy";

describe("private route proxy", () => {
  afterEach(() => {
    vi.unstubAllEnvs();
    auth.getClaims.mockReset();
    auth.setCookies.mockReset();
  });

  function request(path: string) {
    vi.stubEnv("NEXT_PUBLIC_SUPABASE_URL", "https://project.supabase.co");
    vi.stubEnv("NEXT_PUBLIC_SUPABASE_PUBLISHABLE_KEY", "sb_publishable_test");
    return new NextRequest(`http://localhost:3000${path}`);
  }

  it("redirects an anonymous deep link to login and preserves its return path", async () => {
    auth.getClaims.mockResolvedValue({ data: null });

    const response = await proxy(request("/internal/system-status?tab=worker"));
    const location = new URL(response.headers.get("location")!);

    expect(response.status).toBe(307);
    expect(location.pathname).toBe("/login");
    expect(location.searchParams.get("next")).toBe(
      "/internal/system-status?tab=worker",
    );
  });

  it("also protects configuration routes and preserves their return path", async () => {
    auth.getClaims.mockResolvedValue({ data: null });

    const response = await proxy(request("/configuracion/usuarios?estado=activo"));
    const location = new URL(response.headers.get("location")!);

    expect(response.status).toBe(307);
    expect(location.pathname).toBe("/login");
    expect(location.searchParams.get("next")).toBe(
      "/configuracion/usuarios?estado=activo",
    );
  });

  it("lets an authenticated user reach the requested internal page", async () => {
    auth.getClaims.mockResolvedValue({ data: { claims: { sub: "user-id" } } });

    const response = await proxy(request("/internal/system-status"));

    expect(response.status).toBe(200);
    expect(response.headers.get("location")).toBeNull();
  });

  it("preserves Supabase cookie mutations on an anonymous redirect", async () => {
    auth.getClaims.mockImplementation(async () => {
      auth.setCookies([
        {
          name: "sb-auth-token",
          value: "",
          options: { maxAge: 0, path: "/" },
        },
      ]);
      return { data: null };
    });

    const response = await proxy(request("/internal"));

    expect(response.status).toBe(307);
    expect(response.headers.get("set-cookie")).toContain("sb-auth-token=");
    expect(response.headers.get("set-cookie")).toContain("Max-Age=0");
  });

  it("does not allow invalid or expired claims through private route protection", async () => {
    auth.getClaims.mockResolvedValue({ data: null });

    const response = await proxy(request("/internal"));

    expect(response.status).toBe(307);
    expect(auth.getClaims).toHaveBeenCalledOnce();
  });
});
