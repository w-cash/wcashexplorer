import { NetworkPage } from '@/components/explorer-pages';
import { ExplorerShell } from '@/components/explorer-shell';

export default function Page() {
  return (
    <ExplorerShell>
      <NetworkPage />
    </ExplorerShell>
  );
}
