import { BlocksPage } from '@/components/explorer-pages';
import { ExplorerShell } from '@/components/explorer-shell';

export default function Page() {
  return (
    <ExplorerShell>
      <BlocksPage />
    </ExplorerShell>
  );
}
