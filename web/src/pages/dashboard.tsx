export function Dashboard() {
  return (
    <div className="space-y-6">
      <header>
        <h2 className="text-xl font-semibold text-(--color-fg)">Dashboard</h2>
        <p className="mt-1 text-sm text-(--color-muted)">
          The SPA shell is up. Routes, data loaders, and screens land in
          subsequent tickets (221–224, then the feature stories).
        </p>
      </header>
      <section className="grid grid-cols-1 gap-4 md:grid-cols-2 lg:grid-cols-3">
        {["Stores", "Wrappers", "Operators"].map((title) => (
          <div
            key={title}
            className="rounded-xl border border-(--color-line) bg-(--color-surface-0)/60 p-5"
          >
            <div className="text-xs uppercase tracking-wide text-(--color-muted)">
              {title}
            </div>
            <div className="mt-3 text-2xl font-semibold text-(--color-fg)">—</div>
          </div>
        ))}
      </section>
    </div>
  );
}
