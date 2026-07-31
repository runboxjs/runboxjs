# RunboxJS npm Publish Guide

This project publishes the JavaScript package as `runboxjs` from `runbox/pkg`.
The recommended flow is the scripted build/publish pipeline in `publisher/build.mjs`.

## Prerequisites

- npm account with publish permissions
- `npm login` completed locally
- Rust + `wasm-pack` installed
- clean working tree recommended

## Version Sources

- Rust crate version: `Cargo.toml`
- npm package template: `publisher/pkg-template.json`
- generated publish manifest: `pkg/package.json`

`publisher/build.mjs` synchronizes versions and writes `pkg/package.json` from `publisher/pkg-template.json`.

## Recommended Publish Flow

### 1. Build (no bump)

```bash
node publisher/build.mjs
```

This will:

1. read versions from Cargo and existing `pkg/package.json`
2. choose highest valid semver as base
3. run `wasm-pack build --target web --release`
4. regenerate `pkg/package.json` from template

### 2. Bump version (optional)

```bash
node publisher/build.mjs --bump patch
# or --bump minor / --bump major
```

### 3. Publish

```bash
node publisher/build.mjs --publish
```

Or combine bump + publish in two explicit steps:

```bash
node publisher/build.mjs --bump patch
node publisher/build.mjs --publish
```

## Manual Verification Checklist

Before publishing, verify:

- `pkg/package.json` has `name: runboxjs`
- version is the expected one
- `pkg/runbox.js` exists
- `pkg/runbox_bg.wasm` exists
- `pkg/runbox.d.ts` exists

You can inspect package contents with:

```bash
cd pkg
npm pack --dry-run
```

## Common Release Issues

### Version goes backwards

Run the scripted flow from repo root (`node publisher/build.mjs`) so it compares Cargo/package versions and uses the highest semver baseline.

### wasm-pack failure: crate-type must be cdylib

Ensure `Cargo.toml` contains:

```toml
[lib]
crate-type = ["cdylib", "rlib"]
```

### Wrong package name in generated pkg/package.json

`publisher/pkg-template.json` is source of truth. Keep `name` as `runboxjs`.

### npm auth errors

Re-run:

```bash
npm login
npm whoami
```

## Post-Publish Checks

```bash
npm view runboxjs version
npm view runboxjs versions --json
```

Then test install in a clean sample app.
