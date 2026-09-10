'use client';

import { ArrowRight, Search } from 'lucide-react';
import { type SyntheticEvent, useState } from 'react';
import { ExplorerApiError, resolveSearch } from '@/lib/explorer-data';

export function SearchBox() {
  const [query, setQuery] = useState('');
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);

  async function submit(event: SyntheticEvent<HTMLFormElement>) {
    event.preventDefault();
    const value = query.trim();
    if (!value) return;
    setBusy(true);
    setError('');
    try {
      const route = await resolveSearch(value);
      window.location.assign(route);
    } catch (caught) {
      setError(
        caught instanceof ExplorerApiError && caught.status === 404
          ? 'No exact block, transaction, or transparent address match was found.'
          : 'Search is unavailable because the explorer API could not be reached.',
      );
      setBusy(false);
    }
  }

  return (
    <div id="search">
      <search>
        <form
          onSubmit={submit}
          className="panel flex min-h-[64px] items-center gap-3 p-2 pl-4 focus-within:border-[var(--brand)]"
        >
          <Search
            size={21}
            className="shrink-0 text-[var(--brand)]"
            aria-hidden="true"
          />
          <label htmlFor="chain-search" className="sr-only">
            Search by block height, block hash, transaction ID, or transparent
            address
          </label>
          <input
            id="chain-search"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Block height, hash, transaction ID, or Wcash transparent address"
            autoComplete="off"
            spellCheck={false}
            className="mono min-w-0 flex-1 bg-transparent py-3 text-sm text-[var(--text)] outline-none placeholder:font-sans placeholder:text-[var(--faint)]"
          />
          <button
            type="submit"
            disabled={busy || !query.trim()}
            className="flex min-h-11 items-center gap-2 rounded-[10px] bg-[var(--brand)] px-4 text-sm font-extrabold text-[var(--brand-ink)] disabled:cursor-not-allowed disabled:opacity-50"
          >
            <span className="hidden sm:inline">
              {busy ? 'Checking' : 'Explore'}
            </span>
            <ArrowRight size={17} aria-hidden="true" />
          </button>
        </form>
      </search>
      <p
        aria-live="polite"
        className="mt-2 min-h-5 px-1 text-sm text-[var(--danger)]"
      >
        {error}
      </p>
    </div>
  );
}
