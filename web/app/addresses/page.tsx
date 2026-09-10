import type { Metadata } from 'next';
import { AddressesPage } from '@/components/explorer-analytics';
import { ExplorerShell } from '@/components/explorer-shell';

export const metadata: Metadata = {
  title: 'Transparent balances',
  description:
    'Canonical Wcash transparent-address activity and balances. Shielded identities and balances are not public chain data.',
};

export default function Page() {
  return (
    <ExplorerShell>
      <AddressesPage />
    </ExplorerShell>
  );
}
