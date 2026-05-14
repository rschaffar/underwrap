type Metric = {
  label: string;
  value: string | number;
  tone?: 'default' | 'good' | 'warn';
};

export function MetricGrid({ metrics }: { metrics: Metric[] }) {
  return (
    <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
      {metrics.map((metric) => {
        const toneClass =
          metric.tone === 'good'
            ? 'border-emerald-400/20 bg-emerald-400/8 text-emerald-100'
            : metric.tone === 'warn'
              ? 'border-amber-400/20 bg-amber-400/8 text-amber-100'
              : 'border-white/10 bg-black/20 text-white';
        return (
          <div key={metric.label} className={`rounded-2xl border p-4 ${toneClass}`}>
            <div className="text-xs uppercase tracking-[0.25em] text-[var(--muted)]">{metric.label}</div>
            <div className="mt-3 text-2xl font-semibold tracking-tight">{metric.value}</div>
          </div>
        );
      })}
    </div>
  );
}
