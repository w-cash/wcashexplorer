export type Amount = {
  zatoshi: string;
  decimal: string;
  symbol: string;
};

export type MergeMiningSummary = {
  exactWitnessState: string;
  localValidationState: string;
  parentBlockHash: string | null;
  parentLookupState: string;
  parentSourcesAgree: boolean;
  parentHeight: number | null;
  parentConfirmations: number | null;
};

export type BlockSummary = {
  height: number;
  hash: string;
  witnessHash: string;
  time: string;
  sizeBytes: number;
  transactionCount: number;
  bits: string;
  difficulty: string;
  reward: Amount;
  confirmations: number;
  mergeMining: MergeMiningSummary;
};

export type Status = {
  networkName: string;
  symbol: string;
  status: string;
  indexedHeight: number | null;
  nodeHeight: number | null;
  lagBlocks: number | null;
  updatedAt: string;
  difficulty: string | null;
  observedSpacingSeconds: number | null;
  targetSpacingSeconds: number;
  totalIssued: Amount;
  maxSupply: Amount;
  initialSubsidy: Amount;
  nextHalvingHeight: number;
  coinbaseMaturity: number;
};

export type Envelope<T> = {
  data: T;
  meta: {
    nextCursor?: string | null;
    indexedHeight: number | null;
    nodeHeight: number | null;
    freshnessSeconds: number | null;
    network: string;
    requestId?: string;
  };
};

export type DashboardData = {
  status: Envelope<Status>;
  blocks: Envelope<BlockSummary[]>;
  proofBlock?: BlockSummary;
  source: 'live' | 'preview' | 'unavailable';
};

export type ValuePool = {
  id: string;
  chainValue: Amount | null;
  valueDelta: Amount | null;
  monitored: boolean | null;
};

export type TransactionSummary = {
  txid: string;
  authDigest: string;
  version: number;
  sizeBytes: number;
  isCoinbase: boolean;
  kind: 'coinbase' | 'transparent' | 'shielded' | 'mixed';
  fee: Amount | null;
  publicOutputValue: Amount;
  valueBalance: Amount | null;
  transparentInputCount: number;
  transparentOutputCount: number;
  saplingSpendCount: number;
  saplingOutputCount: number;
  orchardActionCount: number;
  ironwoodActionCount: number;
  blockHeight: number;
  blockHash: string;
  blockTime: string;
  position: number;
};

export type ParentObservation = {
  sourceName: string;
  observationState: string;
  parentHeight: number | null;
  parentConfirmations: number | null;
  parentTime: string | null;
  parentBits: string | null;
  parentDifficultyText: string | null;
  embeddedHeaderMatches: boolean | null;
  checkedAt: string;
};

export type AuxPowEvidence = {
  proofVersion: number;
  proofSize: number;
  witnessHash: string;
  exactWitnessState: string;
  witnessConfirmations: number | null;
  localValidationState: string;
  verifierVersion: string | null;
  parentBlockHash: string;
  parentHeaderBits: string;
  parentHashMeetsClaimedTarget: boolean;
  parentCoinbaseTxid: string;
  parentMerkleDepth: number;
  parentCoinbaseIndex: number;
  authDataMerkleDepth: number;
  authDataCoinbaseIndex: number;
  auxiliaryMerkleDepth: number;
  auxiliaryIndex: number;
  parentLookupState: string;
  parentSourcesAgree: boolean;
  verifiedAt: string;
  parentBlockUrl: string;
  parentCoinbaseTxUrl: string;
  observations: ParentObservation[];
  meaning: string;
};

export type BlockDetail = BlockSummary & {
  previousBlockHash: string | null;
  nextBlockHash: string | null;
  merkleRoot: string;
  blockCommitments: string | null;
  nonce: string;
  valuePools: ValuePool[];
  transactions: TransactionSummary[];
  auxpow: AuxPowEvidence | null;
};

export type TransactionInput = {
  inputIndex: number;
  previousTxid: string | null;
  previousOutputIndex: number | null;
  coinbaseData: string | null;
  sequence: number | null;
};

export type TransactionOutput = {
  index: number;
  value: Amount;
  address: string | null;
  scriptType: string | null;
  scriptHex: string | null;
};

export type TransactionDetail = TransactionSummary & {
  inputs: TransactionInput[];
  outputs: TransactionOutput[];
  rawRpc: Record<string, unknown>;
  privacyNotice: string;
};

export type AddressDetail = {
  address: string;
  addressType: string;
  totalReceived: Amount;
  unspent: Amount;
  immatureCoinbase: Amount;
  matureCoinbaseMustShield: Amount;
  utxoCount: number;
  minedOutputCount: number;
  coinbaseMaturity: number;
  activity: Array<{
    txid: string;
    authDigest: string;
    blockHeight: number;
    blockHash: string;
    blockTime: string;
    received: Amount;
  }>;
  scopeNotice: string;
};

export type Reorg = {
  eventId: string;
  oldTipHeight: number;
  oldTipHash: string;
  commonAncestorHeight: number | null;
  commonAncestorHash: string | null;
  detectedAt: string;
  completedAt: string | null;
};

const hashes = [
  'cd56c0e4c912e1452b544426ae16f1217eece07b0060f1c015d724dc4207a17c',
  'cde981d7637e467099bf31e7bd269196ecf4b782e40c033b75271f14eec4998c',
  '02e4d99370f9918907399d22b5a8e38239d691bd04f0351d944b307b5dff7b03',
  '8ebcd805dfc1ed645b52967ebefd7566038203e8e87fadcdcfad50685363a514',
  'cbbb3d092763193af31a0fd973ffcba2685ed4164cb2ec5856f2bbe1bbebefa0',
  '12a1263f1e61e4758a1580130172e0b429bc1d4d95e50e92a04a2bea9a934790',
  '87c16298f31c1b38c9066e13cb56620f4676200b4d29d9976cfe13d1e8aaec4c',
  'ca51121c9ee7fe0287750e814a266d15b3f40e440757a8668dba6470eb90db1d',
];

const previewBlocks: BlockSummary[] = hashes.map((hash, index) => ({
  height: 48 - index,
  hash,
  witnessHash: 'not-indexed-in-preview',
  time: index < 5 ? '2026-09-07T23:11:49.000Z' : '2026-09-07T21:41:49.000Z',
  sizeBytes: index === 5 ? 2055 : 1991,
  transactionCount: 1,
  bits: '1e008859',
  difficulty: '1',
  reward: { zatoshi: '625000000', decimal: '6.25000000', symbol: 'tWEC' },
  confirmations: index + 1,
  mergeMining: {
    exactWitnessState: 'best_chain',
    localValidationState: 'auxpow_verified',
    parentBlockHash: null,
    parentLookupState: 'not_found',
    parentSourcesAgree: false,
    parentHeight: null,
    parentConfirmations: null,
  },
}));

const previewProofBlock: BlockSummary = {
  height: 1,
  hash: '79cdcea38a54f99a59adddf124490a65de499e5677e693caa14dcf24b5f96007',
  witnessHash: 'not-indexed-in-preview',
  time: '2026-09-07T09:41:49.000Z',
  sizeBytes: 2054,
  transactionCount: 1,
  bits: '1e008859',
  difficulty: '1',
  reward: { zatoshi: '625000000', decimal: '6.25000000', symbol: 'tWEC' },
  confirmations: 48,
  mergeMining: {
    exactWitnessState: 'best_chain',
    localValidationState: 'auxpow_verified',
    parentBlockHash:
      '0000005d48352ca15798f834f835a9b68e14bda9053c98251124725f8b32ad52',
    parentLookupState: 'canonical',
    parentSourcesAgree: true,
    parentHeight: 4_338_226,
    parentConfirmations: null,
  },
};

export const unavailableDashboard: DashboardData = {
  source: 'unavailable',
  status: {
    data: {
      networkName: 'Wcash Testnet',
      symbol: 'tWEC',
      status: 'unavailable',
      indexedHeight: null,
      nodeHeight: null,
      lagBlocks: null,
      updatedAt: '1970-01-01T00:00:00.000Z',
      difficulty: null,
      observedSpacingSeconds: null,
      targetSpacingSeconds: 75,
      totalIssued: { zatoshi: '0', decimal: '0.00000000', symbol: 'tWEC' },
      maxSupply: {
        zatoshi: '2100000000000000',
        decimal: '21000000.00000000',
        symbol: 'tWEC',
      },
      initialSubsidy: {
        zatoshi: '625000000',
        decimal: '6.25000000',
        symbol: 'tWEC',
      },
      nextHalvingHeight: 1_680_001,
      coinbaseMaturity: 100,
    },
    meta: {
      indexedHeight: null,
      nodeHeight: null,
      freshnessSeconds: null,
      network: 'testnet',
    },
  },
  blocks: {
    data: [],
    meta: {
      indexedHeight: null,
      nodeHeight: null,
      freshnessSeconds: null,
      network: 'testnet',
    },
  },
};

export const previewEnabled =
  process.env.NEXT_PUBLIC_EXPLORER_PREVIEW === 'true';

export const previewDashboard: DashboardData = {
  source: 'preview',
  proofBlock: previewProofBlock,
  status: {
    data: {
      networkName: 'Wcash Testnet',
      symbol: 'tWEC',
      status: 'preview',
      indexedHeight: 48,
      nodeHeight: 48,
      lagBlocks: 0,
      updatedAt: '2026-09-08T00:00:00.000Z',
      difficulty: '1',
      observedSpacingSeconds: null,
      targetSpacingSeconds: 75,
      totalIssued: {
        zatoshi: '30000000000',
        decimal: '300.00000000',
        symbol: 'tWEC',
      },
      maxSupply: {
        zatoshi: '2100000000000000',
        decimal: '21000000.00000000',
        symbol: 'tWEC',
      },
      initialSubsidy: {
        zatoshi: '625000000',
        decimal: '6.25000000',
        symbol: 'tWEC',
      },
      nextHalvingHeight: 1_680_001,
      coinbaseMaturity: 100,
    },
    meta: {
      indexedHeight: 48,
      nodeHeight: 48,
      freshnessSeconds: null,
      network: 'testnet',
    },
  },
  blocks: {
    data: previewBlocks,
    meta: {
      indexedHeight: 48,
      nodeHeight: 48,
      freshnessSeconds: null,
      network: 'testnet',
    },
  },
};

export class ExplorerApiError extends Error {
  constructor(public readonly status: number) {
    super(`Explorer API returned ${status}`);
  }
}

export async function api<T>(
  path: string,
  signal?: AbortSignal,
): Promise<Envelope<T>> {
  const response = await fetch(
    `${process.env.NEXT_PUBLIC_EXPLORER_API_BASE ?? ''}${path}`,
    {
      signal,
      headers: { accept: 'application/json' },
    },
  );
  if (!response.ok) throw new ExplorerApiError(response.status);
  return (await response.json()) as Envelope<T>;
}

export async function loadDashboard(
  signal?: AbortSignal,
): Promise<DashboardData> {
  try {
    const [status, blocks] = await Promise.all([
      api<Status>('/api/v1/status', signal),
      api<BlockSummary[]>('/api/v1/blocks?limit=8', signal),
    ]);
    return { status, blocks, source: 'live' };
  } catch {
    return previewEnabled ? previewDashboard : unavailableDashboard;
  }
}

export const loadBlocks = (cursor?: string, signal?: AbortSignal) =>
  api<BlockSummary[]>(
    `/api/v1/blocks?limit=25${cursor ? `&cursor=${encodeURIComponent(cursor)}` : ''}`,
    signal,
  );

export const loadTransactions = (cursor?: string, signal?: AbortSignal) =>
  api<TransactionSummary[]>(
    `/api/v1/transactions?limit=25${cursor ? `&cursor=${encodeURIComponent(cursor)}` : ''}`,
    signal,
  );

export const loadBlock = (id: string, signal?: AbortSignal) =>
  api<BlockDetail>(`/api/v1/blocks/${encodeURIComponent(id)}`, signal);

export const loadTransaction = (
  txid: string,
  blockHash?: string,
  signal?: AbortSignal,
) =>
  api<TransactionDetail>(
    `/api/v1/transactions/${encodeURIComponent(txid)}${blockHash ? `?block=${encodeURIComponent(blockHash)}` : ''}`,
    signal,
  );

export const loadAddress = (address: string, signal?: AbortSignal) =>
  api<AddressDetail>(
    `/api/v1/addresses/${encodeURIComponent(address)}`,
    signal,
  );

export const loadStatus = (signal?: AbortSignal) =>
  api<Status>('/api/v1/status', signal);

export const loadReorgs = (signal?: AbortSignal) =>
  api<Reorg[]>('/api/v1/reorgs', signal);

export async function resolveSearch(query: string): Promise<string> {
  const response = await api<{ route: string }>(
    `/api/v1/search?q=${encodeURIComponent(query)}`,
  );
  return response.data.route;
}
