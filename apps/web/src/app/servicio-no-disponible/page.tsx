import Link from "next/link";

export default function ServiceUnavailablePage() {
  return (
    <main className="state-page">
      <div>
        <p className="state-eyebrow">Agro Ops</p>
        <h1>Servicio no disponible</h1>
        <p>No pudimos verificar tu acceso. Intentá nuevamente en unos minutos.</p>
        <Link href="/">Volver al inicio</Link>
      </div>
    </main>
  );
}
