import type { Metadata } from 'next';
import { BlockPage } from '@/components/explorer-pages';
import { ExplorerShell } from '@/components/explorer-shell';

type Props = { params: Promise<{ id: string }> };

export async function generateMetadata({ params }: Props): Promise<Metadata> {
  const { id } = await params;
  return {
    title: `Wcash block ${id}`,
    description: `Canonical Wcash block ${id}, transaction data, and exact AuxPoW evidence.`,
    openGraph: { images: [] },
    twitter: { images: [] },
  };
}

export default async function Page({ params }: Props) {
  const { id } = await params;
  return (
    <ExplorerShell>
      <BlockPage id={id} />
    </ExplorerShell>
  );
}
