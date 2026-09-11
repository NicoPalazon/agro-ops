import type { ReactNode } from "react";
import { requireCapability } from "@/lib/auth/authorization";

export default async function TerritoryLayout({
  children,
}: Readonly<{ children: ReactNode }>) {
  await requireCapability("territorio:ver", "/territorio");
  return children;
}
