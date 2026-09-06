import { defineConfig, devices } from "@playwright/test";

function stagingWebBaseUrl(): string {
  const value = process.env.STAGING_WEB_BASE_URL;

  if (!value) {
    throw new Error("STAGING_WEB_BASE_URL is required for staging browser E2E tests.");
  }

  let url: URL;
  try {
    url = new URL(value);
  } catch {
    throw new Error("STAGING_WEB_BASE_URL must be an HTTPS origin.");
  }

  if (
    url.protocol !== "https:" ||
    url.username ||
    url.password ||
    url.pathname !== "/" ||
    url.search ||
    url.hash
  ) {
    throw new Error("STAGING_WEB_BASE_URL must be an HTTPS origin without credentials or a path.");
  }

  return url.origin;
}

export default defineConfig({
  testDir: "./e2e",
  testMatch: "staging-auth.spec.ts",
  timeout: 45_000,
  expect: { timeout: 10_000 },
  fullyParallel: false,
  workers: 1,
  retries: 0,
  outputDir: "test-results",
  use: {
    baseURL: stagingWebBaseUrl(),
    trace: "off",
    video: "off",
    screenshot: "off",
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],
});
