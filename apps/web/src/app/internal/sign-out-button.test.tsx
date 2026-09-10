import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

const formStatus = vi.hoisted(() => vi.fn());

vi.mock("react-dom", async (importOriginal) => ({
  ...(await importOriginal<typeof import("react-dom")>()),
  useFormStatus: formStatus,
}));

import { SignOutButton } from "./sign-out-button";

describe("SignOutButton", () => {
  it("disables submission and shows the pending label", () => {
    formStatus.mockReturnValue({ pending: true });

    const markup = renderToStaticMarkup(<SignOutButton />);

    expect(markup).toContain("disabled");
    expect(markup).toContain("Cerrando sesión...");
  });
});
