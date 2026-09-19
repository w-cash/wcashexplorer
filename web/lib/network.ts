export const explorerNetwork =
  process.env.NEXT_PUBLIC_WCASH_NETWORK === 'mainnet' ? 'mainnet' : 'testnet';

export const networkName =
  explorerNetwork === 'mainnet' ? 'Wcash Mainnet' : 'Wcash Testnet';

export const networkSymbol = explorerNetwork === 'mainnet' ? 'WEC' : 'TWC';

export const explorerOrigin =
  explorerNetwork === 'mainnet'
    ? 'https://wcashexplorer.com'
    : 'https://testnet.wcashexplorer.com';
