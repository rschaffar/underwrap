'use client';

import { useState } from 'react';

import type { UiResourceRevealResponse } from '../lib/server/ui-types';

type RevealControlProps = {
  target: string;
  store: string;
  secretId: string;
  csrfToken: string;
  allowDownload?: boolean;
};

export function RevealControl({ target, store, secretId, csrfToken, allowDownload = false }: RevealControlProps) {
  const [result, setResult] = useState<UiResourceRevealResponse | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  async function onReveal() {
    setLoading(true);
    setError(null);
    setCopied(false);
    try {
      const response = await fetch('/actions/resource/reveal', {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({
          target,
          store,
          secret_id: secretId,
          csrf_token: csrfToken,
        }),
      });
      if (!response.ok) {
        throw new Error('reveal failed');
      }
      setResult((await response.json()) as UiResourceRevealResponse);
    } catch {
      setError('Reveal failed.');
    } finally {
      setLoading(false);
    }
  }

  async function onCopy() {
    if (!result || !result.copy_allowed) {
      return;
    }
    const value = result.kind === 'utf8' ? result.text : result.base64;
    await navigator.clipboard.writeText(value);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  }

  return (
    <div className="grid gap-2">
      <div className="flex flex-wrap gap-2">
        <button
          type="button"
          onClick={onReveal}
          disabled={loading}
          className="rounded-xl border border-cyan-400/30 bg-cyan-400/10 px-3 py-1.5 text-sm text-cyan-100 transition hover:bg-cyan-400/15 disabled:opacity-60"
        >
          {result ? 'Reveal again' : loading ? 'Revealing…' : 'Reveal'}
        </button>
        {result?.copy_allowed ? (
          <button
            type="button"
            onClick={onCopy}
            className="rounded-xl border border-white/10 bg-white/5 px-3 py-1.5 text-sm text-white/90 transition hover:bg-white/10"
          >
            {copied ? 'Copied' : `Copy ${result.kind === 'utf8' ? 'value' : 'base64'}`}
          </button>
        ) : null}
        {allowDownload ? (
          <span className="rounded-xl border border-white/10 bg-white/5 px-3 py-1.5 text-xs text-[var(--muted)]">
            Download uses a POST route with no-store headers.
          </span>
        ) : null}
      </div>
      {error ? <div className="text-xs text-rose-300">{error}</div> : null}
      {result ? (
        result.kind === 'utf8' ? (
          <textarea
            readOnly
            value={result.text}
            rows={Math.min(8, Math.max(3, result.text.split('\n').length))}
            className="w-full rounded-2xl border border-white/10 bg-black/30 px-3 py-2 font-mono text-xs text-cyan-50"
          />
        ) : (
          <div className="rounded-2xl border border-white/10 bg-black/30 p-3">
            <div className="text-xs text-amber-200">Binary material revealed as base64.</div>
            <textarea
              readOnly
              value={result.base64}
              rows={4}
              className="mt-2 w-full rounded-xl border border-white/10 bg-black/40 px-3 py-2 font-mono text-xs text-cyan-50"
            />
          </div>
        )
      ) : (
        <div className="text-xs text-[var(--muted)]">Masked by default. Reveals stay in browser state only.</div>
      )}
    </div>
  );
}
