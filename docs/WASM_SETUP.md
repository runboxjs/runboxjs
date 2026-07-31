# RunboxJS + Vite WASM Setup

RunboxJS now ships in a format that works with Vite without extra WASM plugins.

## Default Setup (no extra config)

```bash
npm install runboxjs
```

```ts
import init, { RunboxInstance } from 'runboxjs';

await init();
const runbox = new RunboxInstance();
```

You do not need `vite-plugin-wasm` for standard client-side Vite usage.

## Why it works

The package is built with `wasm-pack --target web`.
That wrapper initializes wasm using a URL relative to `import.meta.url`, which Vite handles as a normal asset pipeline.

## Minimal Smoke Test

```ts
runbox.write_file('/smoke.js', new TextEncoder().encode("console.log('ok');"));
const result = JSON.parse(runbox.exec('node /smoke.js'));
console.log(result.stdout);
```

## Troubleshooting

### Error loading `runbox_bg.wasm`

Clear caches and reinstall:

```bash
rm -rf node_modules dist .vite
npm install
npm run dev
```

### Imported before initialization

Always call `await init()` before `new RunboxInstance()`.

### SSR / non-browser context

RunboxJS targets browser runtime execution. If importing on server-side, guard the import and initialization to client-only code paths.

## Related Docs

- Root README: `./README.md`
- Publish flow: `./NPM_PUBLISH.md`
