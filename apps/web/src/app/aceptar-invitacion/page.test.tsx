import { renderToStaticMarkup } from "react-dom/server";
import { beforeEach, describe, expect, it, vi } from "vitest";

const auth = {
  getUser: vi.fn(),
  setSession: vi.fn(),
  updateUser: vi.fn(),
};

vi.mock("@/lib/supabase/browser", () => ({
  createSupabaseInvitationClient: vi.fn(() => ({ auth })),
}));

import {
  establishInvitationSession,
  INVALID_INVITATION_MESSAGE,
  PASSWORD_POLICY_MESSAGE,
  type InvitationAuthClient,
  UNEXPECTED_INVITATION_MESSAGE,
  updateInvitationPassword,
} from "./invitation-auth";
import AcceptInvitationPage, { InvitationAlert } from "./page";

const USER_A = "11111111-1111-1111-1111-111111111111";
const USER_B = "22222222-2222-2222-2222-222222222222";
const VALID_PASSWORD = "Segura-Agro1!";

function invitationHash(accessToken = "invite-access", refreshToken = "invite-refresh") {
  return `#access_token=${accessToken}&refresh_token=${refreshToken}&type=invite`;
}

function successfulAuth(initialUserId = USER_A) {
  let activeUserId = initialUserId;
  const updatedUserIds: string[] = [];

  const client: InvitationAuthClient = {
    setSession: vi.fn(async () => {
      activeUserId = USER_B;
      return {
        data: { session: { user: { id: USER_B } }, user: { id: USER_B } },
        error: null,
      };
    }),
    getUser: vi.fn(async () => ({
      data: { user: { id: activeUserId } },
      error: null,
    })),
    updateUser: vi.fn(async () => {
      updatedUserIds.push(activeUserId);
      return { data: {}, error: null };
    }),
  };

  return { client, updatedUserIds };
}

describe("aceptación de invitación", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("muestra el formulario con la política vigente", () => {
    const markup = renderToStaticMarkup(<AcceptInvitationPage />);

    expect(markup).toContain("Aceptar invitación");
    expect(markup).toContain("Contraseña");
    expect(markup).toContain("Repetir contraseña");
    expect(markup).toContain(PASSWORD_POLICY_MESSAGE);
    expect(markup).toContain('minLength="12"');
  });

  it("no permite que la sesión cambie de la invitada B a la preexistente A", async () => {
    const { client } = successfulAuth(USER_A);
    const established = await establishInvitationSession(client, invitationHash(), vi.fn());

    expect(established).toEqual({ ok: true, invitedUserId: USER_B });
    if (!established.ok) {
      throw new Error("La sesión de prueba debía quedar establecida");
    }

    vi.mocked(client.getUser).mockResolvedValueOnce({
      data: { user: { id: USER_A } },
      error: null,
    });
    const updated = await updateInvitationPassword(
      client,
      established.invitedUserId,
      VALID_PASSWORD,
    );

    expect(updated).toEqual({ ok: false, error: "unexpected" });
    expect(client.updateUser).not.toHaveBeenCalled();
  });

  it("establece la invitación B y actualiza únicamente a B", async () => {
    const { client, updatedUserIds } = successfulAuth(USER_A);
    const clearCallback = vi.fn();

    const established = await establishInvitationSession(
      client,
      invitationHash(),
      clearCallback,
    );
    expect(established).toEqual({ ok: true, invitedUserId: USER_B });
    expect(clearCallback).toHaveBeenCalledOnce();
    expect(clearCallback.mock.invocationCallOrder[0]).toBeLessThan(
      vi.mocked(client.setSession).mock.invocationCallOrder[0],
    );

    if (!established.ok) {
      throw new Error("La sesión de prueba debía quedar establecida");
    }

    await expect(
      updateInvitationPassword(client, established.invitedUserId, VALID_PASSWORD),
    ).resolves.toEqual({ ok: true });
    expect(updatedUserIds).toEqual([USER_B]);
  });

  it("no llama updateUser para una invitación inválida o vencida", async () => {
    const { client } = successfulAuth();

    const established = await establishInvitationSession(
      client,
      "#error=access_denied&error_code=otp_expired&type=invite",
      vi.fn(),
    );

    expect(established).toEqual({ ok: false, error: "invalid-invitation" });
    expect(client.setSession).not.toHaveBeenCalled();
    expect(client.updateUser).not.toHaveBeenCalled();
  });

  it.each([
    ["menos de 12 caracteres", "Aa1!short"],
    ["sin minúscula", "PASSWORD123!"],
    ["sin mayúscula", "password123!"],
    ["sin número", "PasswordSeguro!"],
    ["sin símbolo", "Password1234"],
  ])("rechaza %s antes de llamar updateUser", async (_case, password) => {
    const { client } = successfulAuth(USER_B);

    await expect(updateInvitationPassword(client, USER_B, password)).resolves.toEqual({
      ok: false,
      error: "password-policy",
    });
    expect(client.getUser).not.toHaveBeenCalled();
    expect(client.updateUser).not.toHaveBeenCalled();
  });

  it("renderiza un rechazo 422 de política como política y no como invitación vencida", async () => {
    const { client } = successfulAuth(USER_B);
    vi.mocked(client.updateUser).mockResolvedValueOnce({
      data: {},
      error: {
        code: "weak_password",
        message: "Password should contain required characters",
        status: 422,
      },
    });

    const result = await updateInvitationPassword(client, USER_B, VALID_PASSWORD);
    const markup = renderToStaticMarkup(<InvitationAlert kind="password-policy" />);

    expect(result).toEqual({ ok: false, error: "password-policy" });
    expect(markup).toContain(PASSWORD_POLICY_MESSAGE);
    expect(markup).not.toContain(INVALID_INVITATION_MESSAGE);
  });

  it("renderiza por separado una invitación inválida", () => {
    const markup = renderToStaticMarkup(<InvitationAlert kind="invalid-invitation" />);

    expect(markup).toContain(INVALID_INVITATION_MESSAGE);
    expect(markup).not.toContain(PASSWORD_POLICY_MESSAGE);
  });

  it("no filtra tokens ni errores crudos en mensajes o logs", async () => {
    const sensitiveAccessToken = ["access", "secret"].join("-");
    const sensitiveRefreshToken = ["refresh", "secret"].join("-");
    const log = vi.spyOn(console, "log").mockImplementation(() => undefined);
    const error = vi.spyOn(console, "error").mockImplementation(() => undefined);
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    const { client } = successfulAuth();
    vi.mocked(client.setSession).mockRejectedValueOnce(
      new Error(`unexpected ${sensitiveAccessToken} ${sensitiveRefreshToken}`),
    );

    const result = await establishInvitationSession(
      client,
      invitationHash(sensitiveAccessToken, sensitiveRefreshToken),
      vi.fn(),
    );
    const markup = renderToStaticMarkup(<InvitationAlert kind="unexpected" />);
    const visibleDiagnostics = JSON.stringify({ result, markup });

    expect(visibleDiagnostics).toContain(UNEXPECTED_INVITATION_MESSAGE);
    expect(visibleDiagnostics).not.toContain(sensitiveAccessToken);
    expect(visibleDiagnostics).not.toContain(sensitiveRefreshToken);
    expect(log).not.toHaveBeenCalled();
    expect(error).not.toHaveBeenCalled();
    expect(warn).not.toHaveBeenCalled();
  });
});
