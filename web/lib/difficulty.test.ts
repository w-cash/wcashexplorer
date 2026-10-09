import assert from 'node:assert/strict';
import test from 'node:test';
import {
  formatEquihashStratumDifficulty,
  WCASH_TO_EQUIHASH_STRATUM_DIFFICULTY,
} from './difficulty.ts';

void test('uses the frozen Wcash-to-Stratum scale', () => {
  assert.equal(WCASH_TO_EQUIHASH_STRATUM_DIFFICULTY, 42.96067089174829);
  assert.equal(
    formatEquihashStratumDifficulty('102.50639733016491'),
    '4403.7436',
  );
});

void test('rejects missing and invalid difficulty values', () => {
  assert.equal(formatEquihashStratumDifficulty(null), null);
  assert.equal(formatEquihashStratumDifficulty('not-a-number'), null);
  assert.equal(formatEquihashStratumDifficulty('-1'), null);
});
