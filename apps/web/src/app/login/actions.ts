"use server";

import { redirect } from "next/navigation";
import { safeReturnPath } from "@/lib/auth/return-path";
import { createSupabaseServerClient } from "@/lib/supabase/server";

export async function login(formData: FormData) {
  const returnPath = safeReturnPath(formData.get("next")?.toString());
  const email = formData.get("email")?.toString().trim();
  const password = formData.get("password")?.toString();

  if (!email || !password) {
    redirect(`/login?next=${encodeURIComponent(returnPath)}&error=invalid_credentials`);
  }

  const supabase = await createSupabaseServerClient();
  const { error } = await supabase.auth.signInWithPassword({ email, password });

  if (error) {
    redirect(`/login?next=${encodeURIComponent(returnPath)}&error=invalid_credentials`);
  }

  redirect(returnPath);
}

export async function logout() {
  const supabase = await createSupabaseServerClient();
  await supabase.auth.signOut();
  redirect("/login");
}
