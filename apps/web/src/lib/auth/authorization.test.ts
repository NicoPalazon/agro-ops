import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  accessToken: vi.fn(),
  redirect: vi.fn((path: string): never => {
    throw new Error(`redirect:${path}`);
  }),
}));

vi.mock("./server", () => ({ authenticatedAccessToken: mocks.accessToken }));
vi.mock("next/navigation", () => ({ redirect: mocks.redirect }));

import { requireCapability } from "./authorization";

describe("web capability gate", () => {
  beforeEach(() => {
    vi.stubEnv("API_BASE_URL", "http://api:8080");
    mocks.accessToken.mockResolvedValue("access-token");
  });

  afterEach(() => {
    vi.unstubAllEnvs();
    vi.unstubAllGlobals();
    vi.clearAllMocks();
  });

  it("allows an enabled user with the required effective permission", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(
        new Response(
          JSON.stringify({
            usuario_id: "user-id",
            organizacion_id: "organization-id",
            permisos: ["consola_tecnica:ver"],
          }),
        ),
      ),
    );

    await expect(
      requireCapability("consola_tecnica:ver", "/internal"),
    ).resolves.toMatchObject({ usuario_id: "user-id" });
    expect(mocks.redirect).not.toHaveBeenCalled();
  });

  it("redirects a session without an Agro Ops mapping to Acceso denegado", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response(null, { status: 403 })));

    await expect(requireCapability("consola_tecnica:ver", "/internal")).rejects.toThrow(
      "redirect:/acceso-denegado",
    );
  });

  it("blocks an enabled user without the requested capability", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(
        new Response(
          JSON.stringify({
            usuario_id: "user-id",
            organizacion_id: "organization-id",
            permisos: ["configuracion:ver"],
          }),
        ),
      ),
    );

    await expect(requireCapability("consola_tecnica:ver", "/internal")).rejects.toThrow(
      "redirect:/acceso-denegado",
    );
  });

  it("distinguishes an unavailable authorization dependency from denial", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response(null, { status: 503 })));

    await expect(requireCapability("consola_tecnica:ver", "/internal")).rejects.toThrow(
      "redirect:/servicio-no-disponible",
    );
  });

  it("keeps the login redirect when no session exists", async () => {
    mocks.accessToken.mockResolvedValue(null);

    await expect(
      requireCapability("consola_tecnica:ver", "/internal/system-status"),
    ).rejects.toThrow("redirect:/login?next=%2Finternal%2Fsystem-status");
  });

  it("blocks configuration without configuracion:administrar", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(
        new Response(
          JSON.stringify({
            usuario_id: "user-id",
            organizacion_id: "organization-id",
            permisos: ["consola_tecnica:ver"],
          }),
        ),
      ),
    );

    await expect(
      requireCapability("configuracion:administrar", "/configuracion/usuarios"),
    ).rejects.toThrow("redirect:/acceso-denegado");
  });
});
