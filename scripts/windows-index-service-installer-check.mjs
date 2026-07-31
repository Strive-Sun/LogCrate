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

function macroBody(source, name) {
  const match = source.match(new RegExp(`!macro ${name}\\r?\\n([\\s\\S]*?)!macroend`));
  assert(match, `NSIS must define ${name}`);
  return match[1];
}

const nsisPreinstall = macroBody(nsis, 'NSIS_HOOK_PREINSTALL');
const nsisPostinstall = macroBody(nsis, 'NSIS_HOOK_POSTINSTALL');

assert(
  nsisPreinstall.includes(
    '!insertmacro CheckIfAppIsRunning "${MAINBINARYNAME}.exe" "${PRODUCTNAME}"',
  ),
  'NSIS must confirm the application is closed before changing the existing service',
);
assert(
  nsisPreinstall.includes(
    'IfFileExists "$INSTDIR\\logcrate_index_service.exe" 0 logcrate_preinstall_done',
  ),
  'NSIS must inspect the existing service executable before copying the replacement',
);
assert(
  nsisPreinstall.includes('nsExec::ExecToLog \'"$SYSDIR\\sc.exe" query "LogCrateIndex"\''),
  'NSIS must check whether the existing index service is registered',
);
assert(
  nsisPreinstall.includes(
    'nsExec::ExecToLog \'"$INSTDIR\\logcrate_index_service.exe" --uninstall\'',
  ),
  'NSIS must stop and remove the registered old service before replacing its executable',
);
assert(
  nsisPreinstall.includes('${If} $0 == "error"') && nsisPreinstall.includes('${ElseIf} $0 != 0'),
  'NSIS pre-install must handle service stop execution and non-zero failures',
);
assert(
  (nsisPreinstall.match(/MessageBox MB_OK\|MB_ICONSTOP/g) ?? []).length === 2 &&
    (nsisPreinstall.match(/^\s*Abort\s*$/gm) ?? []).length === 2,
  'NSIS pre-install service stop failures must block file replacement',
);

assert(
  nsisPostinstall.includes(
    'nsExec::ExecToLog \'"$INSTDIR\\logcrate_index_service.exe" --install\'',
  ),
  'NSIS must execute the installed index service with the fixed --install argument',
);
assert(
  nsisPostinstall.includes('${If} $0 == "error"'),
  'NSIS must handle service installer execution errors',
);
assert(
  nsisPostinstall.includes('${ElseIf} $0 != 0'),
  'NSIS must handle non-zero service installer results',
);
assert(
  (nsisPostinstall.match(/MessageBox MB_OK\|MB_ICONSTOP/g) ?? []).length === 2,
  'NSIS failure branches must display blocking error messages',
);
assert(
  (nsisPostinstall.match(/^\s*Abort\s*$/gm) ?? []).length === 2,
  'Both NSIS post-install failure branches must abort installation',
);
assert(
  /\$\{EndIf\}\r?\n$/.test(nsisPostinstall),
  'A zero service installer result must complete the NSIS post-install hook successfully',
);
assert(
  !/\$0 != 0[\s\S]{0,200}DetailPrint[^\n]*\r?\n\s*\$\{EndIf\}/.test(nsisPostinstall),
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
