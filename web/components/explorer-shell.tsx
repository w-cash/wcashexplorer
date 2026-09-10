'use client';

import Image from 'next/image';
import Link from 'next/link';
import { usePathname } from 'next/navigation';
import { Search } from 'lucide-react';
import type { ReactNode } from 'react';

const navigation = [
  { href: '/blocks', label: 'Blocks' },
  { href: '/txs', label: 'Transactions' },
  { href: '/merge-mining', label: 'Merge mining' },
  { href: '/network', label: 'Network' },
];

function isActive(pathname: string, href: string) {
  if (pathname === href || pathname.startsWith(`${href}/`)) return true;
  if (href === '/blocks' && pathname.startsWith('/block/')) return true;
  return href === '/txs' && pathname.startsWith('/tx/');
}

export function ExplorerShell({ children }: { children: ReactNode }) {
  const pathname = usePathname();

  return (
    <div className="min-h-screen">
      <a
        href="#content"
        className="fixed left-3 top-3 z-50 -translate-y-24 rounded bg-[var(--brand)] px-4 py-3 font-semibold text-[var(--brand-ink)] focus:translate-y-0"
      >
        Skip to explorer data
      </a>
      <header className="sticky top-0 z-40 border-b border-[var(--border)] bg-[var(--canvas)]">
        <div className="mx-auto flex min-h-14 max-w-[1280px] items-center gap-4 px-4 sm:px-6">
          <Link
            href="/"
            className="flex min-h-11 items-center gap-2.5"
            aria-label="WcashExplorer home"
          >
            <Image
              src="/wcash-mark.svg"
              width={27}
              height={27}
              alt=""
              priority
            />
            <span className="text-sm font-semibold tracking-[-0.02em] sm:text-base">
              WcashExplorer
            </span>
          </Link>
          <span className="rounded border border-[var(--border)] px-1.5 py-1 text-[0.64rem] font-semibold text-[var(--muted)] sm:px-2 sm:text-[0.66rem]">
            Testnet
            <span className="hidden sm:inline">
              {' '}
              · coins have no monetary value
            </span>
          </span>
          <nav
            aria-label="Main navigation"
            className="ml-auto hidden self-stretch lg:flex"
          >
            {navigation.map((item) => {
              const active = isActive(pathname, item.href);
              return (
                <Link
                  key={item.href}
                  href={item.href}
                  className={`nav-link ${active ? 'nav-link-active' : ''}`}
                  aria-current={active ? 'page' : undefined}
                >
                  {item.label}
                </Link>
              );
            })}
          </nav>
          <Link
            href="/#search"
            className="ml-auto flex min-h-9 min-w-9 items-center justify-center rounded border border-[var(--border)] text-[var(--muted)] hover:border-[var(--border-strong)] hover:text-[var(--text)] lg:ml-1"
            aria-label="Search the Wcash blockchain"
          >
            <Search size={16} aria-hidden="true" />
          </Link>
        </div>
        <nav
          aria-label="Mobile navigation"
          className="mobile-nav mx-auto flex max-w-[1280px] overflow-x-auto border-t border-[var(--border)] px-4 lg:hidden"
        >
          {navigation.map((item) => {
            const active = isActive(pathname, item.href);
            return (
              <Link
                key={item.href}
                href={item.href}
                aria-current={active ? 'page' : undefined}
                className={`nav-link shrink-0 ${active ? 'nav-link-active' : ''}`}
              >
                {item.label}
              </Link>
            );
          })}
        </nav>
      </header>
      <main
        id="content"
        className="mx-auto max-w-[1280px] px-4 pb-16 pt-7 sm:px-6 sm:pt-9"
      >
        {children}
      </main>
      <footer className="border-t border-[var(--border)]">
        <div className="mx-auto flex max-w-[1280px] flex-col justify-between gap-2 px-4 py-6 text-xs text-[var(--faint)] sm:flex-row sm:px-6">
          <span>Wcash Testnet explorer</span>
          <span>
            Shielded identities and balances are not public chain data.
          </span>
        </div>
      </footer>
    </div>
  );
}
