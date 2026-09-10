export function formatDiagnosticTimestamp(value: string | null): string {
  if (!value) return "—";

  const date = new Date(value);
  return Number.isNaN(date.valueOf())
    ? value
    : new Intl.DateTimeFormat("es-AR", {
        dateStyle: "short",
        timeStyle: "medium",
        timeZone: "America/Argentina/Buenos_Aires",
      }).format(date);
}
