## ADDED Requirements
### Requirement: 前端代码格式化
前端代码库 SHALL 配置统一的格式化工具(Prettier),提供格式化与校验脚本,并在 CI 中校验格式一致性。

#### Scenario: 本地格式化
- **WHEN** 开发者运行格式化脚本
- **THEN** 前端源码按统一规则被格式化

#### Scenario: CI 校验格式
- **WHEN** CI 运行格式校验步骤且存在未格式化代码
- **THEN** 校验失败并指出不合规文件

### Requirement: 前端静态检查
前端代码库 SHALL 配置 ESLint(含 TypeScript 与 React Hooks 规则),提供 lint 脚本,并在 CI 中执行。

#### Scenario: 本地 lint
- **WHEN** 开发者运行 lint 脚本
- **THEN** 工具报告违反规则的代码位置

#### Scenario: CI 执行 lint
- **WHEN** CI 运行 lint 步骤且存在违规
- **THEN** 构建失败并输出违规详情

### Requirement: UI 基元策略
项目 SHALL 明确 UI 基元策略:当前沿用手写轻量组件(菜单、弹层等),暂不引入 Radix/shadcn;该策略 SHALL 记录于项目约定文档,便于后续按交互复杂度重新评估。

#### Scenario: 策略可查
- **WHEN** 贡献者查阅项目约定文档
- **THEN** 能明确得知当前不依赖 Radix、交互组件为手写实现
