import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { mkdtemp, mkdir, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';

import { verifyUpdateMirror } from './update-mirror-verify.mjs';

async function fixture() {
  const root = await mkdtemp(path.join(os.tmpdir(), 'logcrate-update-mirror-verify-'));
  const expectedDirectory = path.join(root, 'expected');
  const assetDirectory = path.join(expectedDirectory, 'releases', 'v1.2.3');
  const assetName = 'LogCrate_1.2.3_x64-setup.exe';
  await mkdir(assetDirectory, { recursive: true });
  await writeFile(path.join(assetDirectory, assetName), 'signed-nsis');
  return { root, expectedDirectory, assetName };
}

async function serve(run) {
  const server = createServer(run);
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  const address = server.address();
  return {
    baseUrl: `http://127.0.0.1:${address.port}`,
    close: () =>
      new Promise((resolve, reject) =>
        server.close((error) => (error ? reject(error) : resolve())),
      ),
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

test('verifies the public manifest, cache policy and mirrored NSIS bytes', () =>
  withFixture(async (value) => {
    let manifest;
    const service = await serve((request, response) => {
      if (request.url.startsWith('/latest.json')) {
        response.writeHead(200, {
          'content-type': 'application/json',
          'cache-control': 'no-store',
        });
        response.end(JSON.stringify(manifest));
      } else if (request.url.startsWith(`/releases/v1.2.3/${value.assetName}`)) {
        response.writeHead(200, { 'cache-control': 'public, max-age=31536000, immutable' });
        response.end('signed-nsis');
      } else {
        response.writeHead(404).end();
      }
    });
    try {
      manifest = {
        version: '1.2.3',
        platforms: {
          'windows-x86_64-nsis': {
            signature: 'signature',
            url: `${service.baseUrl}/releases/v1.2.3/${value.assetName}`,
          },
        },
      };
      await writeFile(
        path.join(value.expectedDirectory, 'latest.json'),
        `${JSON.stringify(manifest)}\n`,
      );
      const result = await verifyUpdateMirror({
        expectedDirectory: value.expectedDirectory,
        baseUrl: service.baseUrl,
        runId: 'test',
      });
      assert.equal(result.version, '1.2.3');
      assert.equal(result.verifiedAssets.length, 1);
      assert.match(result.verifiedAssets[0].sha256, /^[a-f0-9]{64}$/);
    } finally {
      await service.close();
    }
  }));

test('accepts only a 404 as the fallback-only public state', () =>
  withFixture(async (value) => {
    const service = await serve((_request, response) => response.writeHead(404).end());
    try {
      assert.deepEqual(
        await verifyUpdateMirror({
          expectedDirectory: value.expectedDirectory,
          baseUrl: service.baseUrl,
          fallbackOnly: true,
        }),
        { fallbackOnly: true, verifiedAssets: [] },
      );
    } finally {
      await service.close();
    }
  }));

test('rejects stale manifests and altered mirrored bytes', async (context) => {
  await context.test('stale manifest', () =>
    withFixture(async (value) => {
      const expected = { version: '1.2.3', platforms: {} };
      await writeFile(path.join(value.expectedDirectory, 'latest.json'), JSON.stringify(expected));
      const service = await serve((_request, response) => {
        response.writeHead(200, {
          'content-type': 'application/json',
          'cache-control': 'no-store',
        });
        response.end(JSON.stringify({ version: '1.2.2', platforms: {} }));
      });
      try {
        await assert.rejects(
          verifyUpdateMirror({
            expectedDirectory: value.expectedDirectory,
            baseUrl: service.baseUrl,
          }),
          /public latest\.json differs/,
        );
      } finally {
        await service.close();
      }
    }),
  );

  await context.test('altered bytes', () =>
    withFixture(async (value) => {
      let manifest;
      const service = await serve((request, response) => {
        if (request.url.startsWith('/latest.json')) {
          response.writeHead(200, {
            'content-type': 'application/json',
            'cache-control': 'no-cache',
          });
          response.end(JSON.stringify(manifest));
        } else {
          response.writeHead(200, { 'cache-control': 'immutable' });
          response.end('altered');
        }
      });
      try {
        manifest = {
          version: '1.2.3',
          platforms: {
            'windows-x86_64-nsis': {
              signature: 'signature',
              url: `${service.baseUrl}/releases/v1.2.3/${value.assetName}`,
            },
          },
        };
        await writeFile(
          path.join(value.expectedDirectory, 'latest.json'),
          JSON.stringify(manifest),
        );
        await assert.rejects(
          verifyUpdateMirror({
            expectedDirectory: value.expectedDirectory,
            baseUrl: service.baseUrl,
          }),
          /SHA-256 mismatch/,
        );
      } finally {
        await service.close();
      }
    }),
  );
});
