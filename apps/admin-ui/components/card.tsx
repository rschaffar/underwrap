import { ReactNode } from 'react';

type CardProps = {
  title: string;
  eyebrow?: string;
  children: ReactNode;
};

export function Card({ title, eyebrow, children }: CardProps) {
  return (
    <section className="rounded-3xl border border-white/10 bg-[var(--surface-1)] p-5 shadow-2xl shadow-black/20">
      {eyebrow ? <div className="text-[11px] font-semibold uppercase tracking-[0.25em] text-[var(--muted)]">{eyebrow}</div> : null}
      <h2 className="mt-2 text-lg font-semibold tracking-tight text-white">{title}</h2>
      <div className="mt-4">{children}</div>
    </section>
  );
}
