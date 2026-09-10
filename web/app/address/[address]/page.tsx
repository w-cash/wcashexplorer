import type { Metadata } from 'next';
import { AddressPage } from '@/components/explorer-pages';
import { ExplorerShell } from '@/components/explorer-shell';

type Props = { params: Promise<{ address: string }> };

export async function generateMetadata({ params }: Props): Promise<Metadata> {
  const { address } = await params;
  return {
    title: `Wcash transparent address ${address}`,
    description: `Public transparent-chain activity for Wcash address ${address}.`,
    openGraph: { images: [] },
    twitter: { images: [] },
  };
}

export default async function Page({ params }: Props) {
  const { address } = await params;
  return (
    <ExplorerShell>
      <AddressPage address={address} />
    </ExplorerShell>
  );
}
