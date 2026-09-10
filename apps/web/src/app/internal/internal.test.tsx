import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import InternalPage from "./page";

describe("Internal Console", () => {
  it("renders the System Status entry point", () => {
    const markup = renderToStaticMarkup(<InternalPage />);

    expect(markup).toContain("Consola técnica");
    expect(markup).toContain("Estado del sistema");
    expect(markup).toContain('href="/internal/system-status"');
    expect(markup).toContain("Jobs");
    expect(markup).toContain('href="/internal/jobs"');
    expect(markup).toContain("Outbox transaccional");
    expect(markup).toContain('href="/internal/outbox"');
    expect(markup).toContain("Auditoría");
    expect(markup).toContain('href="/internal/audit"');
    expect(markup).toContain("Idempotencia");
    expect(markup).toContain('href="/internal/idempotencia"');
  });
});
