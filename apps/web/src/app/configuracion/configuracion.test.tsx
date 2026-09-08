import { renderToStaticMarkup } from "react-dom/server";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  accessToken: vi.fn(),
  loadUsers: vi.fn(),
  loadRoles: vi.fn(),
}));

vi.mock("@/lib/auth/server", () => ({ authenticatedAccessToken: mocks.accessToken }));
vi.mock("@/lib/access-administration/server", () => ({
  loadUsers: mocks.loadUsers,
  loadRoles: mocks.loadRoles,
}));

import RolesPage from "./roles/page";
import UsersPage from "./usuarios/page";

describe("administración normal de accesos", () => {
  beforeEach(() => {
    mocks.accessToken.mockResolvedValue("admin-token");
    mocks.loadRoles.mockResolvedValue({
      roles: [
        {
          id: "role-id",
          nombre: "Encargado",
          descripcion: "Operación diaria",
          activo: true,
          permisos: ["panel:ver"],
        },
      ],
      permisos: [{ codigo: "panel:ver", nombre: "Ver el panel general" }],
    });
    mocks.loadUsers.mockResolvedValue([
      {
        id: "user-id",
        nombre_completo: "Ana Pérez",
        activo: true,
        roles: [{ id: "role-id", nombre: "Encargado" }],
      },
    ]);
  });

  it("renders the basic user invitation, role assignment and deactivation workflow", async () => {
    const markup = renderToStaticMarkup(await UsersPage());

    expect(markup).toContain("Nuevo usuario");
    expect(markup).toContain("Correo electrónico");
    expect(markup).toContain("Nombre completo");
    expect(markup).toContain("Roles asignados");
    expect(markup).toContain("Ana Pérez");
    expect(markup).toContain("Encargado");
    expect(markup).toContain("Deshabilitar");
    expect(markup).toContain("Guardar");
  });

  it("renders role creation, activation and canonical permission membership", async () => {
    const markup = renderToStaticMarkup(await RolesPage());

    expect(markup).toContain("Nuevo rol");
    expect(markup).toContain("Encargado");
    expect(markup).toContain("panel:ver");
    expect(markup).toContain("Desactivar");
    expect(markup).toContain("Guardar");
  });
});
