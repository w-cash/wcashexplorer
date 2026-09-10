import type { Metadata } from 'next';
import { TransactionPage } from '@/components/explorer-pages';
import { ExplorerShell } from '@/components/explorer-shell';

type Props = {
  params: Promise<{ txid: string }>;
  searchParams: Promise<{ block?: string }>;
};

export async function generateMetadata({ params }: Props): Promise<Metadata> {
  const { txid } = await params;
  return {
    title: `Wcash transaction ${txid}`,
    description: `Public structure and block context for Wcash transaction ${txid}.`,
    openGraph: { images: [] },
    twitter: { images: [] },
  };
}

export default async function Page({ params, searchParams }: Props) {
  const [{ txid }, { block: blockHash }] = await Promise.all([
    params,
    searchParams,
  ]);
  return (
    <ExplorerShell>
      <TransactionPage txid={txid} blockHash={blockHash} />
    </ExplorerShell>
  );
}
