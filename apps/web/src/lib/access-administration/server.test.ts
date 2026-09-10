import { afterEach, describe, expect, it, vi } from "vitest";
import { createUser, updateRole } from "./server";

describe("access administration backend client", () => {
  afterEach(() => {
    vi.unstubAllEnvs();
    vi.unstubAllGlobals();
  });

  it("creates users server-side with bearer authentication", async () => {
    vi.stubEnv("API_BASE_URL", "http://api:8080");
    const fetchMock = vi.fn().mockResolvedValue(
      new Response(
        JSON.stringify({ id: "user-id", nombre_completo: "Ana", activo: true, roles: [] }),
        { status: 201 },
      ),
    );
    vi.stubGlobal("fetch", fetchMock);

    await createUser("private-session-token", {
      correo_electronico: "ana@example.com",
      nombre_completo: "Ana Pérez",
      roles_ids: ["role-id"],
    });

    expect(fetchMock).toHaveBeenCalledWith(
      new URL("http://api:8080/configuracion/usuarios"),
      expect.objectContaining({
        method: "POST",
        headers: expect.objectContaining({
          authorization: "Bearer private-session-token",
        }),
      }),
    );
    expect(JSON.parse(fetchMock.mock.calls[0][1].body)).toEqual({
      correo_electronico: "ana@example.com",
      nombre_completo: "Ana Pérez",
      roles_ids: ["role-id"],
    });
  });

  it("updates role permission membership through the protected backend", async () => {
    vi.stubEnv("API_BASE_URL", "http://api:8080");
    const fetchMock = vi.fn().mockResolvedValue(
      new Response(
        JSON.stringify({ id: "role-id", nombre: "Encargado", activo: true, permisos: [] }),
      ),
    );
    vi.stubGlobal("fetch", fetchMock);

    await updateRole("private-session-token", "role-id", {
      permisos: ["panel:ver"],
    });

    expect(fetchMock.mock.calls[0][0].toString()).toBe(
      "http://api:8080/configuracion/roles/role-id",
    );
    expect(fetchMock.mock.calls[0][1].method).toBe("PATCH");
  });
});
