import { cp, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';

const projectRoot = path.resolve(import.meta.dirname, '..');
const sourceNodeModules = path.join(projectRoot, 'node_modules');
const standaloneRoot = path.join(projectRoot, 'dist', 'standalone');
const standaloneNodeModules = path.join(standaloneRoot, 'node_modules');

const readJson = async (file) => JSON.parse(await readFile(file, 'utf8'));
const projectPackage = await readJson(path.join(projectRoot, 'package.json'));

const packageDirectory = (root, packageName) =>
  path.join(root, ...packageName.split('/'));

// Vinext beta.9 copies its own dependencies into the standalone artifact but
// leaves this non-bundled peer behind. Keep the workaround narrow so the
// release does not ship the application's development dependency tree.
const runtimePackageNames = ['react'];
const runtimePackages = {};

await mkdir(standaloneNodeModules, { recursive: true });

for (const packageName of runtimePackageNames) {
  const expectedVersion = projectPackage.dependencies?.[packageName];
  if (!expectedVersion) {
    throw new Error(`${packageName} must be a pinned production dependency.`);
  }
  const sourceDirectory = packageDirectory(sourceNodeModules, packageName);
  const packageJson = await readJson(
    path.join(sourceDirectory, 'package.json'),
  );
  if (packageJson.version !== expectedVersion) {
    throw new Error(
      `${packageName} resolved to ${packageJson.version}, expected ${expectedVersion}.`,
    );
  }
  const destinationDirectory = packageDirectory(
    standaloneNodeModules,
    packageName,
  );
  await rm(destinationDirectory, { recursive: true, force: true });
  await mkdir(path.dirname(destinationDirectory), { recursive: true });
  await cp(sourceDirectory, destinationDirectory, {
    recursive: true,
    dereference: true,
    preserveTimestamps: true,
  });
  runtimePackages[packageName] = packageJson.version;
}

const manifest = {
  schemaVersion: 1,
  packages: runtimePackages,
};

await writeFile(
  path.join(standaloneRoot, 'standalone-runtime.json'),
  `${JSON.stringify(manifest, null, 2)}\n`,
);

console.log(
  `Prepared standalone runtime with ${runtimePackageNames.length} external package.`,
);
