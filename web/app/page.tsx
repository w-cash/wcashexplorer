import { Dashboard } from '@/components/dashboard';
import { ExplorerShell } from '@/components/explorer-shell';

export default function HomePage() {
  return (
    <ExplorerShell>
      <Dashboard />
    </ExplorerShell>
  );
}
