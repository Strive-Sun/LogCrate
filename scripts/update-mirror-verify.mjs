import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

function requireString(value, label) {
  if (typeof value !== 'string' || value.trim() === '') {
    throw new Error(`${label} must be a non-empty string`);
  }
  return value;
}

function cacheBust(url, runId) {
  const value = new URL(url);
  value.searchParams.set('verify', runId || Date.now().toString());
  return value;
}

async function sha256File(filePath) {
  return createHash('sha256')
    .update(await readFile(filePath))
    .digest('hex');
}

async function sha256Response(response) {
  return createHash('sha256')
    .update(Buffer.from(await response.arrayBuffer()))
    .digest('hex');
}

function localPathForUrl(expectedDirectory, baseUrl, assetUrl) {
  const expectedOrigin = new URL(baseUrl);
  const actual = new URL(assetUrl);
  if (actual.origin !== expectedOrigin.origin) {
    throw new Error(`mirrored asset uses unexpected origin: ${actual.origin}`);
  }
  const segments = actual.pathname
    .split('/')
    .filter(Boolean)
    .map((segment) => decodeURIComponent(segment));
  if (segments.length < 3 || segments[0] !== 'releases') {
    throw new Error(`mirrored asset uses unexpected path: ${actual.pathname}`);
  }
  if (segments.some((segment) => segment === '.' || segment === '..' || segment.includes('\\'))) {
    throw new Error(`mirrored asset path is unsafe: ${actual.pathname}`);
  }
  return path.join(expectedDirectory, ...segments);
}

async function verifyOnce({ expectedDirectory, baseUrl, fallbackOnly, runId, fetchImpl }) {
  const latestUrl = cacheBust(`${baseUrl.replace(/\/$/, '')}/latest.json`, runId);
  const latestResponse = await fetchImpl(latestUrl, {
    headers: { 'cache-control': 'no-cache' },
    redirect: 'follow',
  });

  if (fallbackOnly) {
    if (latestResponse.status !== 404) {
      throw new Error(`fallback-only latest.json returned ${latestResponse.status}, expected 404`);
    }
    return { fallbackOnly: true, verifiedAssets: [] };
  }

  if (!latestResponse.ok) {
    throw new Error(`latest.json returned ${latestResponse.status}`);
  }
  const latestCacheControl = latestResponse.headers.get('cache-control') ?? '';
  if (!/no-store|no-cache/i.test(latestCacheControl)) {
    throw new Error(`latest.json cache-control is unsafe: ${latestCacheControl || '<missing>'}`);
  }

  const expectedManifest = JSON.parse(
    await readFile(path.join(expectedDirectory, 'latest.json'), 'utf8'),
  );
  const actualManifest = await latestResponse.json();
  assert.deepEqual(
    actualManifest,
    expectedManifest,
    'public latest.json differs from staged payload',
  );

  const nsisReleases = Object.entries(actualManifest.platforms ?? {}).filter(([platform]) =>
    /^windows-.+-nsis$/.test(platform),
  );
  if (nsisReleases.length === 0) {
    throw new Error('public latest.json does not contain a Windows NSIS platform');
  }

  const verifiedAssets = [];
  for (const assetUrl of [...new Set(nsisReleases.map(([, release]) => release.url))].sort()) {
    const localPath = localPathForUrl(expectedDirectory, baseUrl, assetUrl);
    const response = await fetchImpl(cacheBust(assetUrl, runId), {
      headers: { 'cache-control': 'no-cache' },
      redirect: 'follow',
    });
    if (!response.ok) throw new Error(`mirrored asset returned ${response.status}: ${assetUrl}`);
    const cacheControl = response.headers.get('cache-control') ?? '';
    if (!/immutable/i.test(cacheControl)) {
      throw new Error(
        `mirrored asset cache-control is not immutable: ${cacheControl || '<missing>'}`,
      );
    }
    const [expectedHash, actualHash] = await Promise.all([
      sha256File(localPath),
      sha256Response(response),
    ]);
    if (actualHash !== expectedHash) {
      throw new Error(`mirrored asset SHA-256 mismatch: ${assetUrl}`);
    }
    verifiedAssets.push({ url: assetUrl, sha256: actualHash });
  }

  return { fallbackOnly: false, version: actualManifest.version, verifiedAssets };
}

export async function verifyUpdateMirror({
  expectedDirectory,
  baseUrl,
  fallbackOnly = false,
  runId = '',
  attempts = 1,
  retryDelayMs = 0,
  fetchImpl = fetch,
}) {
  const expected = path.resolve(requireString(expectedDirectory, 'expectedDirectory'));
  const origin = requireString(baseUrl, 'baseUrl').replace(/\/$/, '');
  const parsedOrigin = new URL(origin);
  const isLocalHttp = parsedOrigin.protocol === 'http:' && parsedOrigin.hostname === '127.0.0.1';
  if (parsedOrigin.protocol !== 'https:' && !isLocalHttp) {
    throw new Error('baseUrl must use HTTPS');
  }
  if (!Number.isInteger(attempts) || attempts < 1) throw new Error('attempts must be at least 1');

  let lastError;
  for (let attempt = 1; attempt <= attempts; attempt += 1) {
    try {
      return await verifyOnce({
        expectedDirectory: expected,
        baseUrl: origin,
        fallbackOnly,
        runId: `${runId}-${attempt}`,
        fetchImpl,
      });
    } catch (error) {
      lastError = error;
      if (attempt < attempts) {
        await new Promise((resolve) => setTimeout(resolve, retryDelayMs));
      }
    }
  }
  throw lastError;
}

function parseArguments(args) {
  const options = {};
  for (let index = 0; index < args.length; index += 1) {
    const argument = args[index];
    if (argument === '--fallback-only') {
      options.fallbackOnly = true;
      continue;
    }
    if (!argument.startsWith('--')) throw new Error(`unexpected argument: ${argument}`);
    const value = args[index + 1];
    if (!value || value.startsWith('--')) throw new Error(`missing value for ${argument}`);
    index += 1;
    const key = {
      '--expected-dir': 'expectedDirectory',
      '--base-url': 'baseUrl',
      '--run-id': 'runId',
      '--attempts': 'attempts',
      '--retry-delay-ms': 'retryDelayMs',
    }[argument];
    if (!key) throw new Error(`unknown option: ${argument}`);
    options[key] = key === 'attempts' || key === 'retryDelayMs' ? Number(value) : value;
  }
  return options;
}

const invokedPath = process.argv[1] ? pathToFileURL(path.resolve(process.argv[1])).href : '';
if (import.meta.url === invokedPath) {
  try {
    const result = await verifyUpdateMirror(parseArguments(process.argv.slice(2)));
    console.log(JSON.stringify(result));
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  }
}
