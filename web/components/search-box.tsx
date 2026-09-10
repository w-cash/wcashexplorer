'use client';

import { Search } from 'lucide-react';
import { useState } from 'react';
import type { SyntheticEvent } from 'react';
import { ExplorerApiError, resolveSearch } from '@/lib/explorer-data';

export function SearchBox() {
  const [query, setQuery] = useState('');
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);

  async function handleSubmit(event: SyntheticEvent<HTMLFormElement>) {
    event.preventDefault();
    const normalized = query.trim();
    if (!normalized || loading) return;

    setError('');
    setLoading(true);
    try {
      window.location.assign(await resolveSearch(normalized));
    } catch (failure) {
      setError(
        failure instanceof ExplorerApiError && failure.status === 404
          ? 'No matching block, transaction, or transparent address.'
          : 'Search is temporarily unavailable.',
      );
      setLoading(false);
    }
  }

  return (
    <div id="search">
      <search>
        <form className="search-form" onSubmit={handleSubmit}>
          <Search
            size={16}
            className="shrink-0 text-[var(--faint)]"
            aria-hidden="true"
          />
          <label className="sr-only" htmlFor="chain-search">
            Search blocks, transactions, and transparent addresses
          </label>
          <input
            id="chain-search"
            className="mono min-w-0 flex-1 border-0 bg-transparent text-sm text-[var(--text)] outline-none placeholder:font-sans placeholder:text-[var(--faint)]"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Block, transaction, or transparent address"
            autoComplete="off"
            spellCheck={false}
          />
          <button
            className="search-submit"
            type="submit"
            disabled={!query.trim() || loading}
          >
            {loading ? 'Searching…' : 'Search'}
          </button>
        </form>
      </search>
      {error ? (
        <p className="mt-2 text-xs text-[var(--danger)]" role="alert">
          {error}
        </p>
      ) : null}
    </div>
  );
}
