import { expect, test } from "@playwright/test";

function requiredStagingSecret(name: string): string {
  const value = process.env[name];

  if (!value) {
    throw new Error(`${name} is required for the staging browser E2E test.`);
  }

  return value;
}

test("un enlace privado conserva el destino durante el inicio de sesión", async ({
  page,
}) => {
  const email = requiredStagingSecret("STAGING_SMOKE_EMAIL");
  const password = requiredStagingSecret("STAGING_SMOKE_PASSWORD");

  await page.goto("/internal/system-status");
  await expect(page).toHaveURL(/\/login(?:\?|$)/);

  const loginUrl = new URL(page.url());
  expect(loginUrl.pathname).toBe("/login");
  expect(loginUrl.searchParams.get("next")).toBe("/internal/system-status");

  await page.getByLabel("Correo electrónico").fill(email);
  await page.getByLabel("Contraseña").fill(password);
  await page
    .getByRole("button", { name: "Iniciar sesión", exact: true })
    .click();

  await expect(page).toHaveURL(/\/internal\/system-status$/);
  await expect(
    page.getByRole("heading", { name: "Estado del sistema" }),
  ).toBeVisible();
  await expect(
    page.getByRole("list", { name: "Estado de servicios" }),
  ).toBeVisible();

  for (const service of ["Web", "API", "Base de datos", "Worker"]) {
    await expect(page.getByLabel(`${service}: Operativo`)).toBeVisible();
  }

  await page.reload();

  await expect(page).toHaveURL(/\/internal\/system-status$/);
  await expect(
    page.getByRole("heading", { name: "Estado del sistema" }),
  ).toBeVisible();
  await expect(page.getByLabel("Web: Operativo")).toBeVisible();
});
