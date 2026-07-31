# application-updating Specification

## Purpose
TBD - created by archiving change add-settings-update-check. Update Purpose after archive.
## Requirements
### Requirement: 启动自动检查更新

系统 SHALL 提供默认开启且可持久化的“启动时自动检查更新”开关，并在其开启时于每次应用进程启动后最多检查一次最新正式版本。

#### Scenario: 启动时发现新版本

- **WHEN** 自动检查已开启且应用启动检查发现最新正式版本高于当前版本，且该版本未被跳过
- **THEN** 系统提示当前版本与最新版本，并提供“跳过此版本”和“下载更新”操作

#### Scenario: 启动时已是最新版

- **WHEN** 自动检查已开启且应用启动检查确认当前版本不低于最新正式版本
- **THEN** 系统不显示打扰性提示并保持应用正常可用

#### Scenario: 用户关闭自动检查

- **WHEN** 用户关闭自动检查后重新启动应用
- **THEN** 系统保留关闭状态且不发起启动更新检查

#### Scenario: 自动检查失败

- **WHEN** 启动检查因离线、超时、限流或无效响应失败
- **THEN** 系统不阻断启动、不弹出错误对话框且日志查看功能保持可用

### Requirement: 手动检查更新

系统 SHALL 在设置面板提供手动检查操作，并在每次操作后明确展示检查中、已是最新版、发现新版本或检查失败状态。

#### Scenario: 手动检查发现新版本

- **WHEN** 用户点击“检查更新”且最新正式版本高于当前版本
- **THEN** 系统展示当前版本、最新版本、“跳过此版本”和“下载更新”操作，即使该版本曾被自动检查跳过

#### Scenario: 手动检查已是最新版

- **WHEN** 用户点击“检查更新”且当前版本不低于最新正式版本
- **THEN** 系统明确提示当前已是最新版本

#### Scenario: 手动检查失败

- **WHEN** 用户点击“检查更新”但请求失败或返回无效版本
- **THEN** 系统展示可理解的失败信息并允许用户再次检查

#### Scenario: 防止重复检查

- **WHEN** 一次检查、下载或安装任务仍在进行中
- **THEN** 系统禁用重复触发并展示当前阶段

### Requirement: 跳过指定版本

系统 SHALL 持久化用户跳过的版本，并只抑制该版本的后续自动提示，不影响手动检查或未来更高版本。

#### Scenario: 自动检查不再提示已跳过版本

- **WHEN** 用户选择跳过 `1.2.0`，后续启动检查得到的最新版本仍为 `1.2.0`
- **THEN** 系统不再自动提示该版本

#### Scenario: 更高版本恢复提示

- **WHEN** 用户已跳过 `1.2.0`，后续启动检查发现 `1.3.0`
- **THEN** 系统提示 `1.3.0` 可下载更新

### Requirement: 更新下载进度

系统 SHALL 在用户选择“下载更新”后显示更新包下载进度，并在下载完成时将确定进度显示为 100% 后自动进入安装阶段。

#### Scenario: 已知更新包大小

- **WHEN** updater 提供更新包总字节数并持续返回下载分片
- **THEN** 系统按已下载字节比例更新进度条，下载完成时显示 100%

#### Scenario: 更新包大小未知

- **WHEN** updater 未提供更新包总字节数
- **THEN** 系统显示不确定进度而不伪造百分比，并在下载完成时显示 100%

#### Scenario: 下载失败

- **WHEN** 更新包下载中断或失败
- **THEN** 系统保留当前可运行版本、展示下载失败并允许重试

### Requirement: 签名验证与自动安装

系统 MUST 仅安装通过内置 updater 公钥验证的更新包，并 SHALL 在下载与验证完成后自动安装更新并重启应用。

#### Scenario: 成功安装签名更新

- **WHEN** 用户确认下载且更新包完成下载并通过签名验证
- **THEN** 系统自动安装更新并重启到新版本，无需再次确认

#### Scenario: 拒绝无效签名

- **WHEN** 更新包签名缺失、无效或不匹配内置公钥
- **THEN** 系统拒绝安装、保持当前版本可用并显示验证失败

#### Scenario: 安装失败

- **WHEN** 更新包通过验证但安装过程失败
- **THEN** 系统保持或恢复当前可运行版本，展示安装失败并允许重试

### Requirement: 正式版本与签名发布

系统 SHALL 按固定顺序通过 `https://logcrate-updates.pages.dev/latest.json` 和官方 LogCrate GitHub Releases endpoint 获取最新正式版本。Pages SHALL 提供更新清单和版本化的 Windows NSIS 更新包，Windows URL SHALL 指向同一次 Pages deployment 中的包，macOS URL SHALL 保持指向官方 GitHub Release。新构建 MUST 使用 `Strive-Sun/LogCrate` 规范路径，且 MUST NOT 为未发布的旧品牌客户端保留 updater endpoint 兼容要求。Release 流程 MUST 为 Windows 与 macOS 更新包生成签名和更新清单，MUST 保留现有 updater 签名信任，并且签名私钥与 Cloudflare 凭据 MUST NOT 存储在仓库、构建产物、应用包或日志中。

#### Scenario: Pages 镜像正式版本可更新

- **WHEN** `vX.Y.Z` 正式 tag 的 Release workflow 成功完成
- **THEN** LogCrate GitHub Release 包含更新清单、签名以及 Windows/macOS 可安装更新包，Pages 的同一 production deployment 包含对应清单和 Windows NSIS 包，且两处使用同一版本与签名

#### Scenario: Windows 优先使用 Pages

- **WHEN** Windows 客户端可访问 Pages 且 Pages 返回高于当前版本的有效清单
- **THEN** updater 从清单中的 Pages 版本化 URL 下载 Windows NSIS 包，并仍使用应用内置公钥验证原签名

#### Scenario: Pages 不可用时回退 GitHub

- **WHEN** Pages endpoint 因网络请求失败、非成功响应或 JSON 对象不符合 updater release schema 而不可用
- **THEN** updater 继续尝试固定的官方 GitHub Releases endpoint，且不降低签名验证要求

#### Scenario: macOS 包继续由 GitHub 提供

- **WHEN** macOS 客户端从 Pages 获取有效更新清单
- **THEN** 清单中的 macOS 更新包 URL 指向同版本的官方 GitHub Release，且 updater 验证原签名后安装

#### Scenario: 镜像内容完整后才提供清单

- **WHEN** workflow 为新版本构造 Pages 镜像
- **THEN** workflow 在同一次 production deployment 中上线已验证的 Windows 包与引用该包的 `latest.json`，不会公开引用缺失、错误版本、空签名或非 NSIS 资产的清单

#### Scenario: GitHub 发布前禁用旧镜像清单

- **WHEN** workflow 已验证 draft Release 并准备公开比现有 Pages 清单更新的版本
- **THEN** workflow 先部署不含有效 `latest.json` 的 fallback-only 站点并验证其不可作为更新清单，随后才公开 GitHub Release

#### Scenario: 镜像发布失败时保持安全回退

- **WHEN** GitHub Release 公开后，完整 Pages deployment 或公网验证失败
- **THEN** workflow 明确失败，Pages 保持 fallback-only 状态而不恢复旧清单，客户端可通过第二 endpoint 获取当前 GitHub Release

#### Scenario: Pages 单文件限制

- **WHEN** 待镜像的 Windows NSIS 包大于或等于 25 MiB
- **THEN** workflow 在生成完整 Pages deployment 前明确失败并报告实际大小，不发布部分镜像或引用该包的清单

#### Scenario: 更新清单不长期缓存

- **WHEN** 客户端或中间缓存请求 Pages `latest.json`
- **THEN** 响应要求不存储或每次重新验证，而版本化 Windows 包可使用长期 immutable 缓存

#### Scenario: 签名或 Cloudflare 配置缺失

- **WHEN** Release workflow 缺少 updater 私钥、必需密码、Cloudflare API token 或 account ID
- **THEN** 发布在公开 GitHub Release 或生成不完整 Pages 镜像前明确失败

#### Scenario: 旧客户端迁移限制

- **WHEN** v1.0.21 或更早的已发布客户端检查更新
- **THEN** 该客户端仍只访问其内置 GitHub endpoint，并可通过 GitHub 升级到首个包含双 endpoint 的版本

