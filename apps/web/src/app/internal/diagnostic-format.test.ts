import { describe, expect, it } from "vitest";
import { formatDiagnosticTimestamp } from "./diagnostic-format";

describe("formato de fechas de diagnóstico", () => {
  it("muestra ausencias e entradas inválidas sin inventar una fecha", () => {
    expect(formatDiagnosticTimestamp(null)).toBe("—");
    expect(formatDiagnosticTimestamp("fecha-desconocida")).toBe(
      "fecha-desconocida",
    );
  });

  it("usa la zona horaria operativa argentina", () => {
    expect(formatDiagnosticTimestamp("2026-09-09T15:30:00Z")).toContain(
      "12:30",
    );
  });
});
