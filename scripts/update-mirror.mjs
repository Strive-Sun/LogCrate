import { copyFile, mkdir, readFile, readdir, stat, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

export const PAGES_FILE_LIMIT_BYTES = 25 * 1024 * 1024;
export const DEFAULT_MIRROR_BASE_URL = 'https://logcrate-updates.pages.dev';

const HEADERS = `/latest.json
  Cache-Control: no-store, no-cache, must-revalidate

/releases/*
  Cache-Control: public, max-age=31536000, immutable
`;

function requireString(value, label) {
  if (typeof value !== 'string' || value.trim() === '') {
    throw new Error(`${label} must be a non-empty string`);
  }
  return value;
}

function versionForTag(tag) {
  const match = /^v(.+)$/.exec(requireString(tag, 'tag'));
  if (!match) throw new Error(`tag must start with v: ${tag}`);
  return match[1];
}

function assetNameFromUrl(value, label) {
  const url = new URL(requireString(value, label));
  if (url.protocol !== 'https:') throw new Error(`${label} must use HTTPS`);
  const name = decodeURIComponent(path.posix.basename(url.pathname));
  if (!name || name === '.' || name === '..' || name.includes('/') || name.includes('\\')) {
    throw new Error(`${label} does not contain a safe asset name`);
  }
  return name;
}

async function ensureEmptyDirectory(directory) {
  await mkdir(directory, { recursive: true });
  const entries = await readdir(directory);
  if (entries.length > 0) {
    throw new Error(`output directory must be empty: ${directory}`);
  }
}

async function copySiteDirectory(source, destination) {
  const entries = await readdir(source, { withFileTypes: true });
  for (const entry of entries) {
    if (entry.name === 'latest.json' || entry.name === '_headers' || entry.name === 'releases') {
      throw new Error(`site directory contains reserved mirror path: ${entry.name}`);
    }
    const sourcePath = path.join(source, entry.name);
    const destinationPath = path.join(destination, entry.name);
    if (entry.isDirectory()) {
      await mkdir(destinationPath);
      await copySiteDirectory(sourcePath, destinationPath);
    } else if (entry.isFile()) {
      await copyFile(sourcePath, destinationPath);
    } else {
      throw new Error(`site directory contains unsupported entry: ${sourcePath}`);
    }
  }
}

export async function prepareUpdateMirror({
  outputDirectory,
  siteDirectory,
  fallbackOnly = false,
  manifestPath,
  assetsDirectory,
  tag,
  baseUrl = DEFAULT_MIRROR_BASE_URL,
}) {
  const output = path.resolve(requireString(outputDirectory, 'outputDirectory'));
  const site = path.resolve(requireString(siteDirectory, 'siteDirectory'));
  await ensureEmptyDirectory(output);
  await copySiteDirectory(site, output);
  const notFoundPage = await stat(path.join(output, '404.html')).catch(() => null);
  if (!notFoundPage?.isFile()) {
    throw new Error('site directory must contain 404.html to disable Pages SPA fallback');
  }
  await writeFile(path.join(output, '_headers'), HEADERS, 'utf8');

  if (fallbackOnly) {
    return { fallbackOnly: true, mirroredAssets: [] };
  }

  const expectedVersion = versionForTag(tag);
  const manifest = JSON.parse(
    await readFile(path.resolve(requireString(manifestPath, 'manifestPath')), 'utf8'),
  );
  if (manifest?.version !== expectedVersion) {
    throw new Error(
      `manifest version ${JSON.stringify(manifest?.version)} does not match tag ${tag}`,
    );
  }
  if (!manifest.platforms || typeof manifest.platforms !== 'object') {
    throw new Error('manifest platforms must be an object');
  }

  for (const [platform, release] of Object.entries(manifest.platforms)) {
    if (!release || typeof release !== 'object') {
      throw new Error(`platform ${platform} must be an object`);
    }
    requireString(release.signature, `platform ${platform} signature`);
    const assetName = assetNameFromUrl(release.url, `platform ${platform} url`);
    const assetPath = path.join(
      path.resolve(requireString(assetsDirectory, 'assetsDirectory')),
      assetName,
    );
    const assetStat = await stat(assetPath).catch(() => null);
    if (!assetStat?.isFile()) {
      throw new Error(`missing release asset for platform ${platform}: ${assetName}`);
    }
  }

  const nsisPlatforms = Object.entries(manifest.platforms).filter(([platform]) =>
    /^windows-.+-nsis$/.test(platform),
  );
  if (nsisPlatforms.length === 0) {
    throw new Error('manifest does not contain a Windows NSIS platform');
  }
  if (!Object.keys(manifest.platforms).some((platform) => platform.startsWith('darwin-'))) {
    throw new Error('manifest does not contain a macOS platform');
  }

  const assets = new Map();
  for (const [platform, release] of nsisPlatforms) {
    const assetName = assetNameFromUrl(release.url, `platform ${platform} url`);
    if (!assetName.toLowerCase().endsWith('-setup.exe')) {
      throw new Error(`platform ${platform} does not reference an NSIS setup executable`);
    }
    const assetPath = path.join(path.resolve(assetsDirectory), assetName);
    const assetStat = await stat(assetPath).catch(() => null);
    if (!assetStat?.isFile()) {
      throw new Error(`missing Windows NSIS asset: ${assetName}`);
    }
    if (assetStat.size >= PAGES_FILE_LIMIT_BYTES) {
      throw new Error(
        `Windows NSIS asset ${assetName} is ${assetStat.size} bytes; Pages requires less than ${PAGES_FILE_LIMIT_BYTES} bytes`,
      );
    }
    assets.set(assetName, { source: assetPath, size: assetStat.size });
  }

  const mirrorBaseUrl = requireString(baseUrl, 'baseUrl').replace(/\/$/, '');
  const parsedBaseUrl = new URL(mirrorBaseUrl);
  if (parsedBaseUrl.protocol !== 'https:' || parsedBaseUrl.pathname !== '/') {
    throw new Error('baseUrl must be an HTTPS origin without a path');
  }

  for (const [platform, release] of Object.entries(manifest.platforms)) {
    if (/^windows-.+-nsis$/.test(platform)) {
      const assetName = assetNameFromUrl(release.url, `platform ${platform} url`);
      release.url = `${mirrorBaseUrl}/releases/${encodeURIComponent(tag)}/${encodeURIComponent(assetName)}`;
    }
  }

  const genericWindowsPlatforms = Object.entries(manifest.platforms).filter(
    ([platform, release]) =>
      /^windows-[^-]+$/.test(platform) &&
      assets.has(assetNameFromUrl(release.url, `platform ${platform} url`)),
  );
  for (const [platform, release] of genericWindowsPlatforms) {
    const assetName = assetNameFromUrl(release.url, `platform ${platform} url`);
    release.url = `${mirrorBaseUrl}/releases/${encodeURIComponent(tag)}/${encodeURIComponent(assetName)}`;
  }

  const releaseDirectory = path.join(output, 'releases', tag);
  await mkdir(releaseDirectory, { recursive: true });
  for (const [assetName, asset] of [...assets.entries()].sort(([left], [right]) =>
    left.localeCompare(right),
  )) {
    await copyFile(asset.source, path.join(releaseDirectory, assetName));
  }
  await writeFile(
    path.join(output, 'latest.json'),
    `${JSON.stringify(manifest, null, 2)}\n`,
    'utf8',
  );

  return {
    fallbackOnly: false,
    mirroredAssets: [...assets.entries()].map(([name, asset]) => ({ name, size: asset.size })),
  };
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
      '--output-dir': 'outputDirectory',
      '--site-dir': 'siteDirectory',
      '--manifest': 'manifestPath',
      '--assets-dir': 'assetsDirectory',
      '--tag': 'tag',
      '--base-url': 'baseUrl',
    }[argument];
    if (!key) throw new Error(`unknown option: ${argument}`);
    options[key] = value;
  }
  return options;
}

const invokedPath = process.argv[1] ? pathToFileURL(path.resolve(process.argv[1])).href : '';
if (import.meta.url === invokedPath) {
  try {
    const result = await prepareUpdateMirror(parseArguments(process.argv.slice(2)));
    console.log(JSON.stringify(result));
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  }
}
