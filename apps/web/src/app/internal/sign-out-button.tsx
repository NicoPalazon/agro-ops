"use client";

import { useFormStatus } from "react-dom";

export function SignOutButton() {
  const { pending } = useFormStatus();

  return (
    <button disabled={pending} type="submit">
      {pending ? "Cerrando sesión..." : "Cerrar sesión"}
    </button>
  );
}
