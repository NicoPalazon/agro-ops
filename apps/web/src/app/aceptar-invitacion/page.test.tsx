import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

vi.mock("@/lib/supabase/browser", () => ({
  createSupabaseBrowserClient: vi.fn(() => ({
    auth: { updateUser: vi.fn(), signOut: vi.fn() },
  })),
}));

import AcceptInvitationPage from "./page";

describe("aceptación de invitación", () => {
  it("permite definir la contraseña antes del primer inicio de sesión", () => {
    const markup = renderToStaticMarkup(<AcceptInvitationPage />);

    expect(markup).toContain("Aceptar invitación");
    expect(markup).toContain("Contraseña");
    expect(markup).toContain("Repetir contraseña");
    expect(markup).toContain("Guardar contraseña");
  });
});
