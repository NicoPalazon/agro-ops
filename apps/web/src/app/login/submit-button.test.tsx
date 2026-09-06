import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

const formStatus = vi.hoisted(() => vi.fn());

vi.mock("react-dom", async (importOriginal) => ({
  ...(await importOriginal<typeof import("react-dom")>()),
  useFormStatus: formStatus,
}));

import { LoginSubmitButton } from "./submit-button";

describe("LoginSubmitButton", () => {
  it("disables submission and shows the pending label", () => {
    formStatus.mockReturnValue({ pending: true });

    const markup = renderToStaticMarkup(<LoginSubmitButton />);

    expect(markup).toContain("disabled");
    expect(markup).toContain("Signing in...");
  });
});
