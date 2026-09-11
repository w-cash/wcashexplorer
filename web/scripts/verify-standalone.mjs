import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { cp, mkdtemp, readFile, rm } from 'node:fs/promises';
import { createServer } from 'node:net';
import os from 'node:os';
import path from 'node:path';

const projectRoot = path.resolve(import.meta.dirname, '..');
const standaloneRoot = path.join(projectRoot, 'dist', 'standalone');
const temporaryRoot = await mkdtemp(
  path.join(os.tmpdir(), 'wcashexplorer-standalone-'),
);

const output = [];
let child;

const allocatePort = async () => {
  const server = createServer();
  server.unref();
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  const address = server.address();
  await new Promise((resolve, reject) => {
    server.close((error) => {
      if (error) reject(error);
      else resolve();
    });
  });
  if (!address || typeof address === 'string') {
    throw new Error('Unable to allocate a standalone smoke-test port.');
  }
  return address.port;
};

const stopChild = async () => {
  if (!child || child.exitCode !== null) return;
  child.kill('SIGTERM');
  const stopped = await Promise.race([
    once(child, 'exit').then(() => true),
    new Promise((resolve) => setTimeout(() => resolve(false), 5_000)),
  ]);
  if (!stopped && child.exitCode === null) {
    child.kill('SIGKILL');
    await once(child, 'exit');
  }
};

try {
  await readFile(path.join(standaloneRoot, 'server.js'));
  await readFile(path.join(standaloneRoot, 'standalone-runtime.json'));
  await cp(standaloneRoot, temporaryRoot, {
    recursive: true,
    dereference: true,
  });

  const port = await allocatePort();
  child = spawn(process.execPath, ['server.js'], {
    cwd: temporaryRoot,
    env: {
      HOST: '127.0.0.1',
      NODE_ENV: 'production',
      NODE_NO_WARNINGS: '1',
      PORT: String(port),
    },
    stdio: ['ignore', 'pipe', 'pipe'],
  });

  for (const stream of [child.stdout, child.stderr]) {
    stream.setEncoding('utf8');
    stream.on('data', (chunk) => {
      output.push(chunk);
      if (output.join('').length > 16_384) output.shift();
    });
  }

  const origin = `http://127.0.0.1:${port}`;
  const deadline = Date.now() + 15_000;
  let ready = false;
  while (Date.now() < deadline && child.exitCode === null) {
    try {
      const response = await fetch(origin, {
        signal: AbortSignal.timeout(1_000),
      });
      ready = response.status === 200;
      if (ready) break;
    } catch {
      // The process can take a moment to bind on slower release builders.
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }

  if (!ready) {
    throw new Error(
      `Standalone server did not become ready.\n${output.join('')}`,
    );
  }

  const routes = [
    '/',
    '/blocks',
    '/txs',
    '/addresses',
    '/network',
    '/merge-mining',
    '/block/48',
    `/tx/${'0'.repeat(64)}`,
    '/address/WT8ZbkEWkWb7sU2iniCE7H5KARkUZUkjsFZ',
    '/og.png',
    '/wcash-mark.svg',
  ];

  for (const route of routes) {
    const response = await fetch(`${origin}${route}`, {
      signal: AbortSignal.timeout(3_000),
    });
    if (response.status !== 200) {
      throw new Error(`${route} returned HTTP ${response.status}.`);
    }
    await response.arrayBuffer();
  }

  if (/ERR_MODULE_NOT_FOUND|Cannot find package/.test(output.join(''))) {
    throw new Error(
      `Standalone runtime dependency failure.\n${output.join('')}`,
    );
  }

  console.log(
    `Verified standalone runtime from an isolated directory (${routes.length} routes).`,
  );
} finally {
  await stopChild();
  await rm(temporaryRoot, { recursive: true, force: true });
}
