import { expect, test } from "@playwright/test";

function requiredStagingSecret(name: string): string {
  const value = process.env[name];

  if (!value) {
    throw new Error(`${name} is required for the staging browser E2E test.`);
  }

  return value;
}

test("an anonymous System Status deep link authenticates through the login UI and retains its session", async ({
  page,
}) => {
  const email = requiredStagingSecret("STAGING_SMOKE_EMAIL");
  const password = requiredStagingSecret("STAGING_SMOKE_PASSWORD");

  await page.goto("/internal/system-status");
  await expect(page).toHaveURL(/\/login(?:\?|$)/);

  const loginUrl = new URL(page.url());
  expect(loginUrl.pathname).toBe("/login");
  expect(loginUrl.searchParams.get("next")).toBe("/internal/system-status");

  await page.getByLabel("Email").fill(email);
  await page.getByLabel("Password").fill(password);
  await page.getByRole("button", { name: "Sign in", exact: true }).click();

  await expect(page).toHaveURL(/\/internal\/system-status$/);
  await expect(page.getByRole("heading", { name: "System Status" })).toBeVisible();
  await expect(page.getByRole("list", { name: "Service status" })).toBeVisible();

  for (const service of ["Web", "API", "Database", "Worker"]) {
    await expect(page.getByLabel(`${service}: Operational`)).toBeVisible();
  }

  await page.reload();

  await expect(page).toHaveURL(/\/internal\/system-status$/);
  await expect(page.getByRole("heading", { name: "System Status" })).toBeVisible();
  await expect(page.getByLabel("Web: Operational")).toBeVisible();
});
