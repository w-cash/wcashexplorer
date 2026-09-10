import type { Metadata, Viewport } from 'next';
import './globals.css';

export const metadata: Metadata = {
  metadataBase: new URL('https://wcashexplorer.com'),
  title: {
    default: 'WcashExplorer — Wcash Testnet',
    template: '%s · WcashExplorer',
  },
  description:
    'Browse Wcash Testnet blocks, transactions, transparent addresses, network status, and AuxPoW data.',
  applicationName: 'WcashExplorer',
  icons: { icon: '/wcash-mark.svg' },
  openGraph: {
    title: 'WcashExplorer',
    description:
      'Browse Wcash Testnet blocks, transactions, transparent addresses, network status, and AuxPoW data.',
    type: 'website',
    url: 'https://wcashexplorer.com',
    siteName: 'WcashExplorer',
    images: [
      {
        url: '/og.png',
        width: 1200,
        height: 630,
        alt: 'WcashExplorer Wcash Testnet block explorer',
      },
    ],
  },
  twitter: {
    card: 'summary_large_image',
    title: 'WcashExplorer',
    description: 'Wcash Testnet blocks and Zcash AuxPoW evidence.',
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
