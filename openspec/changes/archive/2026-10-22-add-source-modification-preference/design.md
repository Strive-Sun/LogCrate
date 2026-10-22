## Context

- 修订检测：`App.tsx` 每 2 秒及窗口获得焦点时对活动源调用 `fileRevision`（`len:mtime`），目录监控事件也会触发检查；修订变化时把 `modified` 提示加入队列，同源提示由 `mergeSourceChangePrompt` 合并。
- 现有重载：`reloadSource` 对活动条目调用 `openEntry(force)`，先关闭旧会话、把选项卡置为 `opening`（`session: null`），再打开新会话；同源非活动条目转为 `dormant`。
- `LogContent` 在 `session` 为空时提前返回占位，滚动容器卸载；会话变化的 effect 会把滚动置顶并清空字段条件、模式与“显示未解析”。所选编码仅保存在组件内 ref，会话变化后通过 `rebuildForEncoding` 恢复，而该过程同样会置顶并清空字段条件。
- 前端 `closeLogSession(entryKey, expectedSessionId)` 在映射已指向其它会话时只关闭后端旧会话，不影响新映射；后端 `open_log_session` 每次创建独立会话。

## Goals / Non-Goals

- Goals：提供三种外部修改处理偏好；持续写入的日志可以在自动重载下连续阅读；任何模式都能手动重新加载；不改 IPC 与 Rust。
- Non-Goals：增量追加读取或真正的 tail 跟随（仍为整文件重新索引）；DOCX 预览的无缝重载与视图保留；保留非活动同源条目的视图（它们仍按现有语义转为休眠）；保留查找栏输入与文本选区；跨重启保存视图。

## Decisions

- 偏好以 `logcrate.sourceModifiedAction.v1` 保存在 WebView `localStorage`，取值 `ask | reload | ignore`，读取失败、缺失或未知值回退 `ask`；写入失败时保留当前进程内选择，与界面模板偏好一致。
- 分派点放在修订比较之后、入队提示之前：`ask` 入队提示；`ignore` 直接记录新修订（与“保持当前内容”同一更新）；`reload` 调用重载。偏好通过 ref 读取，修改立即生效，已在队列中的提示不受影响。
- 无缝重载：`openEntry` 增加 `replace` 选项。选项卡已有日志会话时，先打开新会话并取得修订，代际仍有效则一次性替换 `session` 与 `sourceRevision`，随后以旧 `sessionId` 关闭旧会话；失败时保持旧会话与 `ready` 状态并把错误交给调用方。DOCX 会话和无会话选项卡沿用现有 `force` 流程。
- `LogContent` 以“同一 `activeKey` 下由非空旧会话直接切换到不同 `sessionId` 的新会话”识别无缝重载，不新增 props。切换前在 ref 中保存视图快照：首个可见视图行对应的原始行号、是否距底部不超过一屏、`scrollLeft`、字段布局、字段条件、显示模式、`includeUnparsed`。新会话就绪后：
  - 若用户选择过非检测编码，先完成编码重建再应用快照；
  - 复用内存中的字段布局，跳过存储布局指纹核对与快速分析，以保存的条件调用 `applyFieldFilter`；布局应用失败时清空条件并保持可用；
  - 根据快照恢复滚动：跟随末尾时滚动到新的末尾（索引增长期间持续保持末尾），否则定位到保存行号（超出新行数时夹到末尾）。
- 单源串行：`reloadSource` 内按源维护在途 Promise，自动、提示框与手动三种入口统一经过它。在途期间到达的自动请求直接丢弃，由完成后的下一次轮询基于新修订判断；在途期间到达的提示框或手动请求合并为一次待办，在当前重载结束后再执行一次。任意时刻同一源最多一个 `openEntry(replace)` 在途。
- 自动重载失败记录失败修订，相同修订不再重试、不弹错误；提示框与手动重载失败仍沿用现有错误提示。
- DOCX 预览不在本变更的视图保留范围内：DOCX 选项卡仍沿用现有 `force` 重载流程（含加载占位），但同样受偏好分派与单源串行约束，也可从右键菜单手动重新加载。
- 手动入口：`LogTabs` 增加 `onContextMenu`，复用 `ContextMenu` 渲染“重新加载”“关闭”；重新加载调用 `reloadSource(源, undefined)`，与提示框“重新加载”同路径。删除状态的选项卡“重新加载”不可用。

## Risks / Trade-offs

- 大文件在自动模式下每次修订都会整文件重新索引，CPU 与磁盘开销高 → 单源单在途限制，并在设置说明中提示适合中小型持续写入日志。
- 重载期间临时多一个会话，可能触发会话容量淘汰 → 新会话就绪后立即关闭旧会话，淘汰仍走现有 `evictedSessionIds` 处理。
- 文件被截断或轮转后行号不再对应 → 行号夹到新范围；字段条件失效时清空而不是报错阻塞。
- 以会话切换形态隐式识别无缝重载，依赖 App 只有 `replace` 路径会产生“非空到非空”切换 → 由 App 与 LogContent 回归测试共同锁定。

## Migration Plan

新增偏好缺失时即为 `ask`，现有用户行为不变；回滚只需移除偏好读取，存储键残留无害。
