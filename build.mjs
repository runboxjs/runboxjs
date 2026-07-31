#!/usr/bin/env node
/**
 * Build script for runboxjs.
 * Usage: node build.mjs [--bump patch|minor|major] [--publish]
 */

import { execSync } from 'child_process';
import { readFileSync, writeFileSync } from 'fs';

function parseSemver(value, source) {
  const match = /^(\d+)\.(\d+)\.(\d+)$/.exec(String(value).trim());
  if (!match) {
    console.error(`Invalid semver in ${source}: ${value}`);
    process.exit(1);
  }
  return match.slice(1).map(Number);
}

function compareSemver(a, b) {
  for (let i = 0; i < 3; i += 1) {
    if (a[i] > b[i]) return 1;
    if (a[i] < b[i]) return -1;
  }
  return 0;
}

function parseBumpArg(argv) {
  const inline = argv.find((a) => a.startsWith('--bump='));
  if (inline) return inline.split('=')[1];

  const index = argv.indexOf('--bump');
  if (index >= 0) return argv[index + 1];

  return null;
}

const cargoToml = readFileSync('Cargo.toml', 'utf8');
const cargoMatch = cargoToml.match(/^version\s*=\s*"([^"]+)"/m);
if (!cargoMatch) {
  console.error('No version found in Cargo.toml');
  process.exit(1);
}

const cargoVersion = parseSemver(cargoMatch[1], 'Cargo.toml');

let pkgVersion = null;
try {
  const pkgJson = JSON.parse(readFileSync('pkg/package.json', 'utf8'));
  pkgVersion = parseSemver(pkgJson.version, 'pkg/package.json');
} catch {
  // pkg/package.json may not exist before the first wasm-pack build.
}

const baseVersion = pkgVersion && compareSemver(pkgVersion, cargoVersion) > 0
  ? pkgVersion
  : cargoVersion;

let [major, minor, patch] = baseVersion;

const bumpArg = parseBumpArg(process.argv);
if (bumpArg && !['patch', 'minor', 'major'].includes(bumpArg)) {
  console.error(`Invalid bump type: ${bumpArg}. Use patch, minor, or major.`);
  process.exit(1);
}

if (bumpArg === 'major') {
  major += 1;
  minor = 0;
  patch = 0;
} else if (bumpArg === 'minor') {
  minor += 1;
  patch = 0;
} else if (bumpArg === 'patch') {
  patch += 1;
}

const version = `${major}.${minor}.${patch}`;
const nextVersion = [major, minor, patch];

if (compareSemver(nextVersion, cargoVersion) !== 0) {
  const updated = cargoToml.replace(/^(version\s*=\s*)"[^"]+"/m, `$1"${version}"`);
  writeFileSync('Cargo.toml', updated);
  if (bumpArg) {
    console.log(`Version bumped -> ${version}`);
  } else {
    console.log(`Cargo version synced -> ${version}`);
  }
} else if (bumpArg) {
  console.log(`Version unchanged -> ${version}`);
}

console.log('Building WASM...');
execSync('wasm-pack build --target web --release', { stdio: 'inherit' });

const template = JSON.parse(readFileSync('pkg-template.json', 'utf8'));
template.version = version;
writeFileSync('pkg/package.json', `${JSON.stringify(template, null, 2)}\n`);
console.log(`pkg/package.json -> runboxjs@${version}`);

if (process.argv.includes('--publish')) {
  console.log('Publishing to npm...');
  execSync('npm publish --access public', { stdio: 'inherit', cwd: 'pkg' });
  console.log(`runboxjs@${version} published`);
}
