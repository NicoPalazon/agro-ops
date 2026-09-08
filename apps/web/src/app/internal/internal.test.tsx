import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import InternalPage from "./page";

describe("Internal Console", () => {
  it("renders the System Status entry point", () => {
    const markup = renderToStaticMarkup(<InternalPage />);

    expect(markup).toContain("Consola técnica");
    expect(markup).toContain("Estado del sistema");
    expect(markup).toContain('href="/internal/system-status"');
  });
});
