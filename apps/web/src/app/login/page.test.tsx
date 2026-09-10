import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import LoginPage from "./page";

describe("Login page", () => {
  it("offers email/password sign-in without a public sign-up flow", async () => {
    const markup = renderToStaticMarkup(
      await LoginPage({ searchParams: Promise.resolve({}) }),
    );

    expect(markup).toContain('type="email"');
    expect(markup).toContain('type="password"');
    expect(markup).not.toContain("Registrarse");
  });
});
