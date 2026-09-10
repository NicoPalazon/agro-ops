export const PASSWORD_POLICY_MESSAGE =
  "La contraseña debe tener al menos 12 caracteres e incluir mayúscula, minúscula, número y símbolo.";

export const INVALID_INVITATION_MESSAGE =
  "La invitación venció o no es válida. Pedí una nueva invitación.";

export const UNEXPECTED_INVITATION_MESSAGE =
  "No se pudo completar la aceptación de la invitación. Intentá nuevamente.";

const PASSWORD_SYMBOLS = "!@#$%^&*()_+-=[]{};'\\:\"|<>?,./`~";

export type InvitationErrorKind =
  | "password-policy"
  | "invalid-invitation"
  | "unexpected";

type AuthErrorLike = {
  code?: string;
  message?: string;
  name?: string;
  status?: number;
};

type UserIdentity = { id: string };

export type InvitationAuthClient = {
  getUser(): Promise<{
    data: { user: UserIdentity | null };
    error: AuthErrorLike | null;
  }>;
  setSession(session: {
    access_token: string;
    refresh_token: string;
  }): Promise<{
    data: {
      session: { user: UserIdentity } | null;
      user: UserIdentity | null;
    };
    error: AuthErrorLike | null;
  }>;
  updateUser(attributes: { password: string }): Promise<{
    data: unknown;
    error: AuthErrorLike | null;
  }>;
};

type InvitationSessionResult =
  | { ok: true; invitedUserId: string }
  | { ok: false; error: Exclude<InvitationErrorKind, "password-policy"> };

type PasswordUpdateResult =
  | { ok: true }
  | { ok: false; error: InvitationErrorKind };

export function invitationErrorMessage(kind: InvitationErrorKind) {
  switch (kind) {
    case "password-policy":
      return PASSWORD_POLICY_MESSAGE;
    case "invalid-invitation":
      return INVALID_INVITATION_MESSAGE;
    case "unexpected":
      return UNEXPECTED_INVITATION_MESSAGE;
  }
}

export function passwordMeetsPolicy(password: string) {
  return (
    password.length >= 12 &&
    /[a-z]/.test(password) &&
    /[A-Z]/.test(password) &&
    /[0-9]/.test(password) &&
    [...password].some((character) => PASSWORD_SYMBOLS.includes(character))
  );
}

function parseInvitationSession(hash: string) {
  const parameters = new URLSearchParams(hash.startsWith("#") ? hash.slice(1) : hash);

  if (
    parameters.has("error") ||
    parameters.has("error_code") ||
    parameters.has("error_description")
  ) {
    return null;
  }

  const accessToken = parameters.get("access_token");
  const refreshToken = parameters.get("refresh_token");

  if (parameters.get("type") !== "invite" || !accessToken || !refreshToken) {
    return null;
  }

  return { access_token: accessToken, refresh_token: refreshToken };
}

const INVALID_SESSION_CODES = new Set([
  "bad_jwt",
  "invalid_credentials",
  "otp_expired",
  "refresh_token_already_used",
  "refresh_token_not_found",
  "session_not_found",
]);

function isInvalidSessionError(error: AuthErrorLike) {
  return (
    error.status === 401 ||
    error.status === 403 ||
    (typeof error.code === "string" && INVALID_SESSION_CODES.has(error.code)) ||
    error.name === "AuthInvalidJwtError" ||
    error.name === "AuthSessionMissingError"
  );
}

function isPasswordPolicyError(error: AuthErrorLike) {
  if (error.code === "weak_password") {
    return true;
  }

  return (
    error.status === 422 &&
    typeof error.message === "string" &&
    /password/i.test(error.message) &&
    /(at least|characters|contain|length|security|weak)/i.test(error.message)
  );
}

export async function establishInvitationSession(
  auth: InvitationAuthClient,
  callbackHash: string,
  clearCallback: () => void,
): Promise<InvitationSessionResult> {
  const invitationSession = parseInvitationSession(callbackHash);
  clearCallback();

  if (!invitationSession) {
    return { ok: false, error: "invalid-invitation" };
  }

  try {
    const established = await auth.setSession(invitationSession);
    if (established.error) {
      return {
        ok: false,
        error: isInvalidSessionError(established.error)
          ? "invalid-invitation"
          : "unexpected",
      };
    }

    const establishedUserId =
      established.data.user?.id ?? established.data.session?.user.id;
    if (!establishedUserId) {
      return { ok: false, error: "unexpected" };
    }

    const verified = await auth.getUser();
    if (verified.error) {
      return {
        ok: false,
        error: isInvalidSessionError(verified.error)
          ? "invalid-invitation"
          : "unexpected",
      };
    }

    if (!verified.data.user || verified.data.user.id !== establishedUserId) {
      return { ok: false, error: "unexpected" };
    }

    return { ok: true, invitedUserId: establishedUserId };
  } catch {
    return { ok: false, error: "unexpected" };
  }
}

export async function updateInvitationPassword(
  auth: InvitationAuthClient,
  invitedUserId: string,
  password: string,
): Promise<PasswordUpdateResult> {
  if (!passwordMeetsPolicy(password)) {
    return { ok: false, error: "password-policy" };
  }

  try {
    const verified = await auth.getUser();
    if (verified.error) {
      return {
        ok: false,
        error: isInvalidSessionError(verified.error)
          ? "invalid-invitation"
          : "unexpected",
      };
    }

    if (!verified.data.user || verified.data.user.id !== invitedUserId) {
      return { ok: false, error: "unexpected" };
    }

    const updated = await auth.updateUser({ password });
    if (!updated.error) {
      return { ok: true };
    }

    if (isPasswordPolicyError(updated.error)) {
      return { ok: false, error: "password-policy" };
    }

    return {
      ok: false,
      error: isInvalidSessionError(updated.error)
        ? "invalid-invitation"
        : "unexpected",
    };
  } catch {
    return { ok: false, error: "unexpected" };
  }
}
