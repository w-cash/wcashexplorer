import type { Metadata, Viewport } from 'next';
import './globals.css';

export const metadata: Metadata = {
  metadataBase: new URL('https://wcashexplorer.com'),
  title: {
    default: 'WcashExplorer — Wcash Testnet',
    template: '%s · WcashExplorer',
  },
  description:
    'Independent Wcash Testnet block explorer with exact AuxPoW verification and Zcash parent-chain evidence.',
  applicationName: 'WcashExplorer',
  icons: { icon: '/wcash-mark.svg' },
  openGraph: {
    title: 'WcashExplorer',
    description:
      'Verify Wcash blocks, transactions, supply, and Zcash merge-mining evidence.',
    type: 'website',
    url: 'https://wcashexplorer.com',
    siteName: 'WcashExplorer',
    images: [
      {
        url: '/og.png',
        width: 1200,
        height: 630,
        alt: 'WcashExplorer — verify the work',
      },
    ],
  },
  twitter: {
    card: 'summary_large_image',
    title: 'WcashExplorer',
    description: 'Verify Wcash blocks and Zcash merge-mining evidence.',
    images: ['/og.png'],
  },
};

export const viewport: Viewport = {
  colorScheme: 'dark light',
  themeColor: [
    { media: '(prefers-color-scheme: dark)', color: '#070b08' },
    { media: '(prefers-color-scheme: light)', color: '#f4f8f3' },
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
