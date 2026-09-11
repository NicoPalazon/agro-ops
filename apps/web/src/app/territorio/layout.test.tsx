import { renderToStaticMarkup } from "react-dom/server";
import { beforeEach, describe, expect, it, vi } from "vitest";

const auth = vi.hoisted(() => ({ requireCapability: vi.fn() }));

vi.mock("@/lib/auth/authorization", () => ({
  requireCapability: auth.requireCapability,
}));

import TerritoryLayout from "./layout";

describe("territorial route authorization", () => {
  beforeEach(() => {
    auth.requireCapability.mockReset();
    auth.requireCapability.mockResolvedValue({});
  });

  it("uses the effective territorio:ver capability for the protected route", async () => {
    const markup = renderToStaticMarkup(
      await TerritoryLayout({ children: <p>Mapa territorial</p> }),
    );

    expect(auth.requireCapability).toHaveBeenCalledWith("territorio:ver", "/territorio");
    expect(markup).toContain("Mapa territorial");
  });
});
