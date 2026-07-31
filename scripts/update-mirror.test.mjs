import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { mkdtemp, mkdir, readFile, rm, truncate, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';

import {
  DEFAULT_MIRROR_BASE_URL,
  PAGES_FILE_LIMIT_BYTES,
  prepareUpdateMirror,
} from './update-mirror.mjs';

const execFileAsync = promisify(execFile);
const scriptPath = fileURLToPath(new URL('./update-mirror.mjs', import.meta.url));

async function fixture() {
  const root = await mkdtemp(path.join(os.tmpdir(), 'logcrate-update-mirror-'));
  const siteDirectory = path.join(root, 'site');
  const assetsDirectory = path.join(root, 'assets');
  const outputDirectory = path.join(root, 'output');
  await Promise.all([
    mkdir(siteDirectory),
    mkdir(assetsDirectory),
    writeFile(path.join(root, 'manifest.json'), ''),
  ]);
  await writeFile(path.join(siteDirectory, 'index.html'), '<p>LogCrate update mirror</p>\n');
  const manifest = {
    version: '1.2.3',
    notes: 'notes',
    pub_date: '2026-07-31T00:00:00Z',
    platforms: {
      'windows-x86_64': {
        signature: 'nsis-signature',
        url: 'https://github.com/Strive-Sun/LogCrate/releases/download/v1.2.3/LogCrate_1.2.3_x64-setup.exe',
      },
      'windows-x86_64-nsis': {
        signature: 'nsis-signature',
        url: 'https://github.com/Strive-Sun/LogCrate/releases/download/v1.2.3/LogCrate_1.2.3_x64-setup.exe',
      },
      'windows-x86_64-msi': {
        signature: 'msi-signature',
        url: 'https://github.com/Strive-Sun/LogCrate/releases/download/v1.2.3/LogCrate_1.2.3_x64_en-US.msi',
      },
      'darwin-aarch64': {
        signature: 'mac-signature',
        url: 'https://github.com/Strive-Sun/LogCrate/releases/download/v1.2.3/LogCrate_universal.app.tar.gz',
      },
    },
  };
  const manifestPath = path.join(root, 'manifest.json');
  await writeFile(manifestPath, `${JSON.stringify(manifest)}\n`);
  await writeFile(path.join(assetsDirectory, 'LogCrate_1.2.3_x64-setup.exe'), 'signed-nsis');
  return {
    root,
    siteDirectory,
    assetsDirectory,
    outputDirectory,
    manifestPath,
    manifest,
  };
}

async function withFixture(run) {
  const value = await fixture();
  try {
    await run(value);
  } finally {
    await rm(value.root, { recursive: true, force: true });
  }
}

function options(value) {
  return {
    outputDirectory: value.outputDirectory,
    siteDirectory: value.siteDirectory,
    manifestPath: value.manifestPath,
    assetsDirectory: value.assetsDirectory,
    tag: 'v1.2.3',
  };
}

test('creates an atomic Pages payload and only rewrites NSIS-compatible Windows URLs', () =>
  withFixture(async (value) => {
    const result = await prepareUpdateMirror(options(value));
    const mirrored = JSON.parse(await readFile(path.join(value.outputDirectory, 'latest.json')));
    const expectedUrl = `${DEFAULT_MIRROR_BASE_URL}/releases/v1.2.3/LogCrate_1.2.3_x64-setup.exe`;

    assert.deepEqual(result, {
      fallbackOnly: false,
      mirroredAssets: [{ name: 'LogCrate_1.2.3_x64-setup.exe', size: 11 }],
    });
    assert.equal(mirrored.platforms['windows-x86_64'].url, expectedUrl);
    assert.equal(mirrored.platforms['windows-x86_64-nsis'].url, expectedUrl);
    assert.equal(
      mirrored.platforms['windows-x86_64-msi'].url,
      value.manifest.platforms['windows-x86_64-msi'].url,
    );
    assert.equal(
      mirrored.platforms['darwin-aarch64'].url,
      value.manifest.platforms['darwin-aarch64'].url,
    );
    assert.equal(mirrored.platforms['windows-x86_64-nsis'].signature, 'nsis-signature');
    assert.equal(
      await readFile(
        path.join(value.outputDirectory, 'releases', 'v1.2.3', 'LogCrate_1.2.3_x64-setup.exe'),
        'utf8',
      ),
      'signed-nsis',
    );
    const headers = await readFile(path.join(value.outputDirectory, '_headers'), 'utf8');
    assert.match(headers, /\/latest\.json\n {2}Cache-Control: no-store, no-cache, must-revalidate/);
    assert.match(headers, /\/releases\/\*\n {2}Cache-Control: public, max-age=31536000, immutable/);
  }));

test('creates a fallback-only payload without latest.json', () =>
  withFixture(async (value) => {
    const result = await prepareUpdateMirror({
      outputDirectory: value.outputDirectory,
      siteDirectory: value.siteDirectory,
      fallbackOnly: true,
    });
    assert.deepEqual(result, { fallbackOnly: true, mirroredAssets: [] });
    await assert.rejects(readFile(path.join(value.outputDirectory, 'latest.json')), /ENOENT/);
    assert.equal(
      await readFile(path.join(value.outputDirectory, 'index.html'), 'utf8'),
      '<p>LogCrate update mirror</p>\n',
    );
  }));

test('supports the fallback-only command-line interface used by the release workflow', () =>
  withFixture(async (value) => {
    const { stdout } = await execFileAsync(process.execPath, [
      scriptPath,
      '--fallback-only',
      '--site-dir',
      value.siteDirectory,
      '--output-dir',
      value.outputDirectory,
    ]);
    assert.deepEqual(JSON.parse(stdout), { fallbackOnly: true, mirroredAssets: [] });
    await assert.rejects(readFile(path.join(value.outputDirectory, 'latest.json')), /ENOENT/);
  }));

test('rejects a manifest version that does not match the tag', () =>
  withFixture(async (value) => {
    await assert.rejects(
      prepareUpdateMirror({ ...options(value), tag: 'v1.2.4' }),
      /does not match tag v1\.2\.4/,
    );
  }));

test('rejects an empty signature on any platform', () =>
  withFixture(async (value) => {
    value.manifest.platforms['darwin-aarch64'].signature = '';
    await writeFile(value.manifestPath, JSON.stringify(value.manifest));
    await assert.rejects(prepareUpdateMirror(options(value)), /darwin-aarch64 signature/);
  }));

test('rejects a missing or non-NSIS Windows asset', async (context) => {
  await context.test('missing asset', () =>
    withFixture(async (value) => {
      await rm(path.join(value.assetsDirectory, 'LogCrate_1.2.3_x64-setup.exe'));
      await assert.rejects(prepareUpdateMirror(options(value)), /missing Windows NSIS asset/);
    }),
  );
  await context.test('non-NSIS URL', () =>
    withFixture(async (value) => {
      value.manifest.platforms['windows-x86_64-nsis'].url =
        'https://github.com/Strive-Sun/LogCrate/releases/download/v1.2.3/LogCrate_1.2.3_x64_en-US.msi';
      await writeFile(value.manifestPath, JSON.stringify(value.manifest));
      await assert.rejects(prepareUpdateMirror(options(value)), /does not reference an NSIS/);
    }),
  );
});

test('accepts the last byte below the Pages limit and rejects the limit itself', async (context) => {
  await context.test('below limit', () =>
    withFixture(async (value) => {
      await truncate(
        path.join(value.assetsDirectory, 'LogCrate_1.2.3_x64-setup.exe'),
        PAGES_FILE_LIMIT_BYTES - 1,
      );
      const result = await prepareUpdateMirror(options(value));
      assert.equal(result.mirroredAssets[0].size, PAGES_FILE_LIMIT_BYTES - 1);
    }),
  );
  await context.test('at limit', () =>
    withFixture(async (value) => {
      await truncate(
        path.join(value.assetsDirectory, 'LogCrate_1.2.3_x64-setup.exe'),
        PAGES_FILE_LIMIT_BYTES,
      );
      await assert.rejects(prepareUpdateMirror(options(value)), /26214400 bytes/);
    }),
  );
});

test('refuses to mix a new payload into a non-empty output directory', () =>
  withFixture(async (value) => {
    await mkdir(value.outputDirectory);
    await writeFile(path.join(value.outputDirectory, 'stale.json'), '{}');
    await assert.rejects(prepareUpdateMirror(options(value)), /output directory must be empty/);
  }));
