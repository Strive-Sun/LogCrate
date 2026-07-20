import { access, readFile } from 'node:fs/promises';
import path from 'node:path';
import { inflateSync } from 'node:zlib';

const root = path.resolve(import.meta.dirname, '..');
const read = (file) => readFile(path.join(root, file), 'utf8');
const readBinary = (file) => readFile(path.join(root, file));
const assert = (condition, message) => {
  if (!condition) throw new Error(message);
};

const decodePng = (buffer, file) => {
  const signature = buffer.subarray(0, 8).toString('hex');
  assert(signature === '89504e470d0a1a0a', `${file} must be a PNG file`);

  let offset = 8;
  let width;
  let height;
  const compressed = [];
  while (offset < buffer.length) {
    const length = buffer.readUInt32BE(offset);
    const type = buffer.toString('ascii', offset + 4, offset + 8);
    const data = buffer.subarray(offset + 8, offset + 8 + length);
    if (type === 'IHDR') {
      width = data.readUInt32BE(0);
      height = data.readUInt32BE(4);
      assert(data[8] === 8 && data[9] === 6, `${file} must use 8-bit RGBA pixels`);
      assert(data[12] === 0, `${file} must not use interlacing`);
    } else if (type === 'IDAT') {
      compressed.push(data);
    } else if (type === 'IEND') {
      break;
    }
    offset += length + 12;
  }

  assert(width && height && compressed.length, `${file} is missing required PNG chunks`);
  const encoded = inflateSync(Buffer.concat(compressed));
  const bytesPerPixel = 4;
  const stride = width * bytesPerPixel;
  const pixels = Buffer.alloc(stride * height);
  const paeth = (left, above, upperLeft) => {
    const estimate = left + above - upperLeft;
    const leftDistance = Math.abs(estimate - left);
    const aboveDistance = Math.abs(estimate - above);
    const upperLeftDistance = Math.abs(estimate - upperLeft);
    if (leftDistance <= aboveDistance && leftDistance <= upperLeftDistance) return left;
    return aboveDistance <= upperLeftDistance ? above : upperLeft;
  };

  for (let y = 0; y < height; y += 1) {
    const sourceStart = y * (stride + 1);
    const filter = encoded[sourceStart];
    for (let x = 0; x < stride; x += 1) {
      const encodedByte = encoded[sourceStart + x + 1];
      const destination = y * stride + x;
      const left = x >= bytesPerPixel ? pixels[destination - bytesPerPixel] : 0;
      const above = y > 0 ? pixels[destination - stride] : 0;
      const upperLeft =
        y > 0 && x >= bytesPerPixel ? pixels[destination - stride - bytesPerPixel] : 0;
      const predictor = [
        0,
        left,
        above,
        Math.floor((left + above) / 2),
        paeth(left, above, upperLeft),
      ][filter];
      assert(predictor !== undefined, `${file} uses an unsupported PNG filter`);
      pixels[destination] = (encodedByte + predictor) & 0xff;
    }
  }

  return { width, height, pixels, stride };
};

const config = JSON.parse(await read('src-tauri/tauri.conf.json'));
assert(config.productName === 'LogCrate', 'Tauri productName must be LogCrate');
assert(config.app.windows[0].title === 'LogCrate', 'Main window title must be LogCrate');
assert(config.identifier === 'com.logpeek.app', 'Legacy bundle identifier must not change');
assert(
  config.plugins.updater.endpoints.includes(
    'https://github.com/Strive-Sun/LogPeek/releases/latest/download/latest.json',
  ),
  'Legacy updater endpoint must remain available during the rename',
);
assert(
  config.plugins.updater.pubkey?.length > 40,
  'Updater signing public key must remain configured',
);
for (const icon of [
  'icons/32x32.png',
  'icons/128x128.png',
  'icons/128x128@2x.png',
  'icons/icon.png',
  'icons/icon.ico',
  'icons/icon.icns',
]) {
  assert(config.bundle.icon.includes(icon), `Tauri bundle must reference ${icon}`);
}

const [app, locale, update, readme] = await Promise.all([
  read('src/App.tsx'),
  read('src/i18n/core.ts'),
  read('src/util/update.ts'),
  read('README.md'),
]);
assert(app.includes("'logpeek.treeWidth'"), 'Legacy tree width key must remain readable');
assert(locale.includes("'logpeek.locale'"), 'Legacy locale key must remain readable');
assert(update.includes("'logpeek.update.autoCheck'"), 'Legacy update setting must remain readable');
assert(readme.includes('<h1 align="center">LogCrate</h1>'), 'README must use LogCrate branding');

for (const icon of [
  'src-tauri/icons/logcrate.svg',
  'src-tauri/icons/icon.ico',
  'src-tauri/icons/icon.icns',
]) {
  await access(path.join(root, icon));
}

for (const [icon, expectedSize] of [
  ['src-tauri/icons/32x32.png', 32],
  ['src-tauri/icons/64x64.png', 64],
  ['src-tauri/icons/128x128.png', 128],
  ['src-tauri/icons/128x128@2x.png', 256],
  ['src-tauri/icons/icon.png', 512],
]) {
  const { width, height, pixels, stride } = decodePng(await readBinary(icon), icon);
  assert(
    width === expectedSize && height === expectedSize,
    `${icon} must be ${expectedSize}x${expectedSize}`,
  );
  const cornerAlpha = [
    pixels[3],
    pixels[stride - 1],
    pixels[(height - 1) * stride + 3],
    pixels.at(-1),
  ];
  assert(
    cornerAlpha.every((alpha) => alpha === 0),
    `${icon} must have transparent outer corners`,
  );
}

console.log('Brand compatibility check passed: LogCrate display identity + LogPeek upgrade keys.');
