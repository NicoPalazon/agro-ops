const DEFAULT_RETURN_PATH = "/internal";
const APPLICATION_ORIGIN = "http://agro-ops.local";

type ReturnPathValue = string | string[] | null | undefined;

function isInternalPathname(pathname: string): boolean {
  return pathname === "/internal" || pathname.startsWith("/internal/");
}

/**
 * Keeps post-login navigation inside the private console. URLs are parsed
 * against a fixed origin so protocol-relative and absolute URLs cannot escape
 * the application.
 */
export function safeReturnPath(value: ReturnPathValue): string {
  if (typeof value !== "string" || !isInternalPathname(value.split(/[?#]/, 1)[0])) {
    return DEFAULT_RETURN_PATH;
  }

  try {
    const url = new URL(value, APPLICATION_ORIGIN);

    return url.origin === APPLICATION_ORIGIN &&
      isInternalPathname(url.pathname) &&
      !/%2f|%5c/i.test(url.pathname)
      ? `${url.pathname}${url.search}`
      : DEFAULT_RETURN_PATH;
  } catch {
    return DEFAULT_RETURN_PATH;
  }
}
