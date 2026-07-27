# LogCrate OpenSpec 规则

本文件说明 LogCrate 执行规则如何与仓库 OpenSpec 流程共同生效，不复制 requirement/scenario 格式或 CLI 手册；具体格式始终以 `openspec/AGENTS.md` 为准。

## 何时读取

每次开启或继续一个需求时都必须先读取本文件，用它判断是否进入完整 OpenSpec 流程。

- 需求需要创建、修改或实施 proposal/spec/change，或者涉及新能力、公共行为变化、破坏性变更、架构调整、重大性能/安全工作或跨模块大范围修复时，继续读取 `openspec/AGENTS.md` 和相关 spec/change。
- 普通问答、只读调查、普通文档修改以及不改变公共行为、架构或规范语义的局部简单修复，无需继续加载任何 OpenSpec 资料，直接进入 Step-1。
- OpenSpec 主要用于需求与变更文档驱动；任务未触发上一条条件时，不得仅因仓库存在 `openspec/` 目录或通用 OpenSpec 文案而扩大读取范围。

## 权威映射

- 现行 `specs/` 描述已生效行为；活动 change 描述拟议变化；`tasks.md` 是该 change 的实施顺序和状态清单。
- `Step-2/工作约束.md` 负责工作状态的使用边界，`Step-4/执行规则.md` 负责任务实施，`Step-5/验证规则.md` 负责验证和证据，`openspec/AGENTS.md` 负责 change 的创建条件、格式、审批、校验和归档。
- `design.md` 已完整记录某项架构决定时直接作为权威记录，不另建内容重复的 ADR。

## Proposal 与实施

- OpenSpec 要求 proposal 的变更，先完成 proposal、必要的 design、tasks 和 delta spec，并通过严格校验。
- 严格校验只证明格式和结构有效，不等于 proposal 已获批准；批准前不得实施产品变更。
- 需要审批的活动 change 必须使用独立的 `approval.md` 保存可从仓库恢复的审批记录。文件缺失、`Status` 不是 `approved`、审批来源不是维护者，或 `Approved-Revision` 与当前已审阅内容不一致时，一律视为 draft。
- Agent 不得自行批准 proposal；只有维护者明确批准后，才能记录 `Approved-By`、`Approved-At`、`Approved-Revision`、`Source` 和审批范围。审批 revision 必须指向包含已审阅 proposal、design、delta specs 以及任务范围和验收定义的已有 commit。
- 审批后若 proposal、design、delta spec、任务范围或验收语义发生变化，必须在同一轮将审批状态退回 draft，重新严格校验并取得新审批。实现代码、测试补充、验证证据和任务勾选未改变上述语义时，不需要重新审批。
- 实施时按 `tasks.md` 顺序执行。每个最小编号的实现任务完成后进入 `规则手册/Step-5/验证规则.md` 规定的验证、交接与提交门禁；全部完成前不得进入后续任务。
- 失败或新发现改变需求、设计、验收语义或后续任务时，先更新 proposal/design/spec/tasks、重新严格校验并取得所需批准，再继续实现。

## 简单修复与无 change 工作

- 加载 OpenSpec 与创建 change 是两道判断：大范围修复需要加载相关 spec/change 核对边界与冲突，但只要它仍是恢复既有规范行为，就可以不创建新 change。
- 局部简单修复无需加载 OpenSpec；大范围修复即使不创建 change，仍需明确范围、核对相关现行 spec 与活动 change，并保留回归测试和验证证据。
- 非破坏性依赖或配置变更只有在不改变公共行为与兼容性、API/schema、安全/隐私/权限边界、持久化与迁移/恢复语义、架构、跨平台行为以及构建/安装/更新/发布产物时，才可以不创建 change；不能仅根据改动文件类型豁免。
- 修复、依赖或配置变更一旦改变上述边界，或出现新能力、架构、重大性能或安全决策，必须升级为 change。

## 完成与归档

- 任务勾选必须反映实际实现与验证状态；不得在末尾批量补勾未经逐项验证的任务。
- OpenSpec 校验、代码测试、真机或性能验收和 proposal 审批是不同门禁，不能互相替代。
- 只有 tasks 与项目要求的验证、部署或用户验收全部完成后才可归档；归档后按项目规则验证现行 specs。

## 冲突

`openspec/AGENTS.md` 可以增加具体要求，但不能静默取消 `Step-4/执行规则.md` 的任务顺序或 `Step-5/验证规则.md` 的验证与证据要求。涉及最小任务拆分、勾选时机、验证、交接、独立 commit 和进入下一任务的条件时，以 Step-4 和 Step-5 为准；不得按通用 OpenSpec 文案在末尾批量补勾任务或合并提交。无法按职责边界消解冲突时，停止实施并请求维护者裁决。
