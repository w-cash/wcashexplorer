'use client';

import Image from 'next/image';
import Link from 'next/link';
import { usePathname } from 'next/navigation';
import {
  Blocks,
  GitMerge,
  RadioTower,
  Search,
  ShieldCheck,
} from 'lucide-react';
import type { ReactNode } from 'react';

const navigation = [
  { href: '/blocks', label: 'Blocks' },
  { href: '/txs', label: 'Transactions' },
  { href: '/merge-mining', label: 'Merge mining' },
  { href: '/network', label: 'Network' },
];

export function ExplorerShell({ children }: { children: ReactNode }) {
  const pathname = usePathname();
  return (
    <div className="min-h-screen">
      <a
        href="#content"
        className="fixed left-3 top-3 z-50 -translate-y-24 rounded-lg bg-[var(--brand)] px-4 py-3 font-bold text-[var(--brand-ink)] focus:translate-y-0"
      >
        Skip to explorer data
      </a>
      <div className="border-b border-[var(--border)] bg-[var(--surface)] text-xs">
        <div className="mx-auto flex min-h-9 max-w-[1440px] flex-wrap items-center justify-between gap-2 px-4 py-2 sm:px-7">
          <div className="flex items-center gap-2 font-semibold text-[var(--warning)]">
            <span className="status-dot" aria-hidden="true" />
            Wcash Testnet · test coins have no monetary value
          </div>
          <div className="flex items-center gap-2 text-[var(--muted)]">
            <RadioTower size={14} aria-hidden="true" />
            <span>Indexer status is shown with every response</span>
          </div>
        </div>
      </div>
      <header className="sticky top-0 z-40 border-b border-[var(--border)] bg-[color-mix(in_srgb,var(--canvas)_90%,transparent)] backdrop-blur-xl">
        <div className="mx-auto flex min-h-[68px] max-w-[1440px] items-center gap-5 px-4 sm:px-7">
          <Link
            href="/"
            className="flex min-h-11 items-center gap-3"
            aria-label="WcashExplorer home"
          >
            <Image
              src="/wcash-mark.svg"
              width={34}
              height={34}
              alt=""
              priority
            />
            <span className="text-base font-extrabold tracking-[-0.03em] sm:text-lg">
              Wcash<span className="text-[var(--brand)]">Explorer</span>
            </span>
          </Link>
          <nav
            aria-label="Main navigation"
            className="ml-auto hidden items-center gap-1 lg:flex"
          >
            {navigation.map((item) => (
              <Link
                key={item.href}
                href={item.href}
                className="flex min-h-11 items-center rounded-lg px-3 text-sm font-semibold text-[var(--muted)] hover:bg-[var(--raised)] hover:text-[var(--text)]"
                aria-current={
                  pathname === item.href || pathname.startsWith(`${item.href}/`)
                    ? 'page'
                    : undefined
                }
              >
                {item.label}
              </Link>
            ))}
          </nav>
          <Link
            href="/#search"
            className="ml-auto flex min-h-11 min-w-11 items-center justify-center rounded-lg border border-[var(--border)] text-[var(--muted)] hover:border-[var(--border-strong)] hover:text-[var(--text)] lg:ml-1"
            aria-label="Search the Wcash blockchain"
          >
            <Search size={18} aria-hidden="true" />
          </Link>
          <span className="pill pill-warning">TESTNET</span>
        </div>
        <nav
          aria-label="Mobile navigation"
          className="mx-auto flex max-w-[1440px] gap-1 overflow-x-auto border-t border-[var(--border)] px-4 lg:hidden"
        >
          {navigation.map((item) => (
            <Link
              key={item.href}
              href={item.href}
              aria-current={
                pathname === item.href || pathname.startsWith(`${item.href}/`)
                  ? 'page'
                  : undefined
              }
              className="flex min-h-11 shrink-0 items-center px-3 text-sm font-semibold text-[var(--muted)] aria-[current=page]:text-[var(--brand)]"
            >
              {item.label}
            </Link>
          ))}
        </nav>
      </header>
      <main
        id="content"
        className="mx-auto max-w-[1440px] px-4 pb-20 pt-8 sm:px-7 sm:pt-12"
      >
        {children}
      </main>
      <footer className="border-t border-[var(--border)] bg-[var(--surface)]">
        <div className="mx-auto grid max-w-[1440px] gap-8 px-4 py-10 sm:grid-cols-3 sm:px-7">
          <div>
            <div className="flex items-center gap-2 font-bold">
              <Blocks
                size={18}
                className="text-[var(--brand)]"
                aria-hidden="true"
              />
              WcashExplorer
            </div>
            <p className="mt-3 max-w-sm text-sm leading-6 text-[var(--muted)]">
              An independent, privacy-honest view of Wcash and its Zcash-format
              AuxPoW evidence.
            </p>
          </div>
          <div className="text-sm">
            <div className="mb-3 flex items-center gap-2 font-bold">
              <ShieldCheck
                size={17}
                className="text-[var(--mint)]"
                aria-hidden="true"
              />{' '}
              Data policy
            </div>
            <p className="leading-6 text-[var(--muted)]">
              This explorer never asks for seed phrases, spending keys, or
              viewing keys.
            </p>
          </div>
          <div className="text-sm">
            <div className="mb-3 flex items-center gap-2 font-bold">
              <GitMerge
                size={17}
                className="text-[var(--info)]"
                aria-hidden="true"
              />{' '}
              Verification
            </div>
            <p className="leading-6 text-[var(--muted)]">
              AuxPoW validity and parent-chain acceptance are reported
              separately.
            </p>
          </div>
        </div>
      </footer>
    </div>
  );
}
