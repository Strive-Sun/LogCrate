import { readFile } from 'node:fs/promises';
import path from 'node:path';

const root = path.resolve(import.meta.dirname, '..');
const read = (file) => readFile(path.join(root, file), 'utf8');
const assert = (condition, message) => {
  if (!condition) throw new Error(message);
};

const [nsis, wix, tauriConfigText, windowsConfigText, cargoManifest] = await Promise.all([
  read('src-tauri/windows/nsis-hooks.nsh'),
  read('src-tauri/windows/index-service.wxs'),
  read('src-tauri/tauri.conf.json'),
  read('src-tauri/tauri.windows.conf.json'),
  read('src-tauri/Cargo.toml'),
]);
const tauriConfig = JSON.parse(tauriConfigText);
const windowsConfig = JSON.parse(windowsConfigText);

assert(
  nsis.includes('nsExec::ExecToLog \'"$INSTDIR\\logcrate_index_service.exe" --install\''),
  'NSIS must execute the installed index service with the fixed --install argument',
);
assert(nsis.includes('${If} $0 == "error"'), 'NSIS must handle service installer execution errors');
assert(nsis.includes('${ElseIf} $0 != 0'), 'NSIS must handle non-zero service installer results');
assert(
  (nsis.match(/MessageBox MB_OK\|MB_ICONSTOP/g) ?? []).length >= 2,
  'NSIS failure branches must display blocking error messages',
);
assert(
  (nsis.match(/^\s*Abort\s*$/gm) ?? []).length === 2,
  'Only the two NSIS failure branches may abort installation',
);
assert(
  /\$\{EndIf\}\r?\n!macroend/.test(nsis),
  'A zero service installer result must complete the NSIS post-install hook successfully',
);
assert(
  !/\$0 != 0[\s\S]{0,200}DetailPrint[^\n]*\r?\n\s*\$\{EndIf\}/.test(nsis),
  'NSIS must not silently continue after only logging a non-zero result',
);

assert(
  /Id="InstallLogCrateIndexService"[\s\S]*?Execute="deferred"[\s\S]*?Impersonate="no"[\s\S]*?Return="check"/.test(
    wix,
  ),
  'MSI service installation must remain elevated, deferred, and rollback on failure',
);
assert(
  wix.includes('ExeCommand="&quot;[INSTALLFOLDER]logcrate_index_service.exe&quot; --install"'),
  'MSI must execute the installed index service with the fixed --install argument',
);
assert(
  /<Custom Action="InstallLogCrateIndexService" After="InstallFiles">/.test(wix),
  'MSI must install the service after its binary is copied',
);

assert(
  tauriConfig.bundle.windows.nsis.installMode === 'perMachine',
  'NSIS must request per-machine installation privileges',
);
assert(
  tauriConfig.bundle.windows.nsis.installerHooks === './windows/nsis-hooks.nsh',
  'Tauri must include the fail-loud NSIS hook',
);
assert(
  tauriConfig.bundle.windows.wix.fragmentPaths.includes('./windows/index-service.wxs'),
  'Tauri must include the MSI service custom action fragment',
);
assert(
  windowsConfig.build.features.includes('windows-index-service'),
  'Windows bundles must enable the index service binary feature',
);
assert(
  /\[\[bin\]\][\s\S]*?name = "logcrate_index_service"[\s\S]*?required-features = \["windows-index-service"\]/.test(
    cargoManifest,
  ),
  'Cargo must declare the version-matched index service binary',
);

console.log('Windows index service installer check passed.');
