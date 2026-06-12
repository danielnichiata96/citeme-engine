import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

// Consumers (CiteMe app) read the wasm binary straight from the package via
// `require.resolve` / `import.meta.resolve`. These resolutions must work from
// the *packed* tarball — the exports map and the "files" allowlist both gate
// them — so the test installs the tarball into a scratch dir and resolves from
// there, exactly like a real install.

const pkgRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const WASM_SUBPATH = 'citeme-engine-wasm/pkg/citeme_engine_wasm_bg.wasm';

function run(cmd, args, opts = {}) {
  return execFileSync(cmd, args, { encoding: 'utf8', ...opts });
}

test('packed tarball resolves root, package.json and wasm subpaths (CJS + ESM)', () => {
  const scratch = mkdtempSync(join(tmpdir(), 'citeme-exports-'));
  try {
    const packOutput = run('npm', ['pack', pkgRoot, '--pack-destination', scratch]);
    const tarball = packOutput.trim().split('\n').pop();
    writeFileSync(
      join(scratch, 'package.json'),
      JSON.stringify({ name: 'scratch', private: true, type: 'commonjs' }),
    );
    run('npm', ['install', '--no-audit', '--no-fund', '--ignore-scripts', join(scratch, tarball)], {
      cwd: scratch,
    });

    const cjsScript = `console.log(JSON.stringify({
      wasm: require.resolve('${WASM_SUBPATH}'),
      manifest: require.resolve('citeme-engine-wasm/package.json'),
      root: require.resolve('citeme-engine-wasm'),
    }))`;
    const cjs = JSON.parse(run('node', ['-e', cjsScript], { cwd: scratch }));
    assert.match(cjs.wasm, /citeme_engine_wasm_bg\.wasm$/);
    assert.match(cjs.manifest, /citeme-engine-wasm[\\/]package\.json$/);
    assert.match(cjs.root, /index\.js$/);

    const esmScript = `console.log(JSON.stringify({
      wasm: import.meta.resolve('${WASM_SUBPATH}'),
      manifest: import.meta.resolve('citeme-engine-wasm/package.json'),
      root: import.meta.resolve('citeme-engine-wasm'),
    }))`;
    const esm = JSON.parse(run('node', ['--input-type=module', '-e', esmScript], { cwd: scratch }));
    assert.match(esm.wasm, /^file:.*citeme_engine_wasm_bg\.wasm$/);
    assert.match(esm.manifest, /^file:.*package\.json$/);
    assert.match(esm.root, /index\.js$/);
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
});
