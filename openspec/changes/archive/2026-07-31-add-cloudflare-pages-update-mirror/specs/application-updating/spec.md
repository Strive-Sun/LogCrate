## MODIFIED Requirements

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
