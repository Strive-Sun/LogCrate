# Change: 后端通知路径应用用户后缀筛选

## Why
基线要求后缀筛选同时作用于目录树展示与"新文件通知",且"不匹配筛选的新文件不提示"。但当前后端检测/通知路径(`classify`/`stable_detect`)使用硬编码的 `is_log_name` 扩展名白名单,完全忽略用户在配置里设置的后缀规则——用户即便把筛选设为仅 `.log`,其它后缀的新文件仍会触发通知。筛选目前只在前端目录树生效,通知与规格不符。

## What Changes
- 后端到达检测在判定"是否计入新日志通知"时 SHALL 应用当前持久化的后缀筛选规则
- `show_all` 开启时不因后缀过滤通知;关闭时仅匹配后缀的新文件才 emit 通知
- 筛选规则变更后对后续到达即时生效(读取最新配置)

## Impact
- Affected specs: `directory-monitoring`(MODIFIED:可配置文件后缀筛选)
- Affected code:
  - `src-tauri/src/watcher.rs`(`classify`/`stable_detect`/detect 回调读取并应用 `suffixes`/`show_all`)
  - `src-tauri/src/lib.rs`(通知 emit 前的筛选判定)
