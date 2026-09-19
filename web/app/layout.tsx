import type { Metadata, Viewport } from 'next';
import './globals.css';
import { explorerOrigin, networkName } from '@/lib/network';

export const metadata: Metadata = {
  metadataBase: new URL(explorerOrigin),
  title: {
    default: `WcashExplorer — ${networkName}`,
    template: '%s · WcashExplorer',
  },
  description:
    `Browse ${networkName} blocks, transactions, transparent addresses, network status, and AuxPoW data.`,
  applicationName: 'WcashExplorer',
  icons: { icon: '/wcash-mark.svg' },
  openGraph: {
    title: 'WcashExplorer',
    description:
      `Browse ${networkName} blocks, transactions, transparent addresses, network status, and AuxPoW data.`,
    type: 'website',
    url: explorerOrigin,
    siteName: 'WcashExplorer',
    images: [
      {
        url: '/og.png',
        width: 1200,
        height: 630,
        alt: `WcashExplorer ${networkName} block explorer`,
      },
    ],
  },
  twitter: {
    card: 'summary_large_image',
    title: 'WcashExplorer',
    description: `${networkName} blocks and locally validated AuxPoW evidence.`,
    images: ['/og.png'],
  },
};

export const viewport: Viewport = {
  colorScheme: 'dark light',
  themeColor: [
    { media: '(prefers-color-scheme: dark)', color: '#0b0e0c' },
    { media: '(prefers-color-scheme: light)', color: '#f4f6f3' },
  ],
};

export default function RootLayout({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}
