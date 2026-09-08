import Link from "next/link";

export default function AccessDeniedPage() {
  return (
    <main className="state-page">
      <div>
        <p className="state-eyebrow">Agro Ops</p>
        <h1>Acceso denegado</h1>
        <p>Tu cuenta no tiene acceso a esta sección.</p>
        <Link href="/">Volver al inicio</Link>
      </div>
    </main>
  );
}
