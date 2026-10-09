// Wcash reports consensus difficulty relative to its frozen Mainnet PoW limit.
// Equihash Stratum software commonly reports difficulty relative to Zcash's
// standard diff-1 target instead. Both values describe the same block target.
//
// standard Equihash diff-1 / Wcash PoW limit:
// (2^243 - 1) / 0x00002fabe80000...0000
export const WCASH_TO_EQUIHASH_STRATUM_DIFFICULTY = 42.96067089174829;

export function formatEquihashStratumDifficulty(
  wcashDifficulty: string | null | undefined,
): string | null {
  if (!wcashDifficulty) return null;

  const parsed = Number(wcashDifficulty);
  if (!Number.isFinite(parsed) || parsed < 0) return null;

  return (parsed * WCASH_TO_EQUIHASH_STRATUM_DIFFICULTY).toFixed(4);
}
