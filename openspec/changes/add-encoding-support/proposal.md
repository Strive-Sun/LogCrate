# Change: 完善编码检测与手动指定编码

## Why
基线要求编码支持覆盖 UTF-8、GBK/GB18030、UTF-16(含 BOM)并允许用户手动指定编码,但当前实现:GB18030 未使用、UTF-16BE 被当作 LE 解码、前端"编码 ▾"仅为静态文本没有交互,也没有重解码命令。非 UTF-8 或 UTF-16BE 日志会乱码且无法纠正。

## What Changes
- 后端编码检测支持 GB18030(GBK 的超集),正确区分 UTF-16LE 与 UTF-16BE(依 BOM)
- 新增命令 `set_session_encoding(session_id, encoding)`:按指定编码重建解码并刷新当前会话
- 前端"编码 ▾"改为可交互下拉:展示检测结果、允许手动切换编码并刷新视图

## Impact
- Affected specs: `log-viewing`(MODIFIED:文本编码检测与解码)
- Affected code:
  - `src-tauri/src/index.rs`(编码检测/解码、按会话重解码)
  - `src-tauri/src/lib.rs`(新增 `set_session_encoding` 命令)
  - `src/api/tauri.ts` / `src/api/mock.ts`(编码接口)
  - `src/components/LogContent.tsx`(编码下拉交互)
