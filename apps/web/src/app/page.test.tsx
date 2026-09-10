import { describe, expect, it, vi } from "vitest";

const navigation = vi.hoisted(() => ({ redirect: vi.fn() }));

vi.mock("next/navigation", () => ({ redirect: navigation.redirect }));

import Home from "./page";

describe("ruta inicial", () => {
  it("envía la entrada pública al flujo protegido de Agro Ops", () => {
    Home();

    expect(navigation.redirect).toHaveBeenCalledExactlyOnceWith("/internal");
  });
});
