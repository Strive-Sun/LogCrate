## 1. 后端筛选应用
- [x] 1.1 到达检测回调读取当前持久化的 `suffixes`/`show_all`
- [x] 1.2 `show_all=false` 时,仅后缀匹配的新文件 emit `new-archive-detected`
- [x] 1.3 `show_all=true` 时不因后缀过滤通知
- [x] 1.4 筛选规则变更后对后续到达即时生效

## 2. 验证
- [x] 2.1 设为仅 `.log` 时,`.tmp`/`.bin` 等新文件不产生通知(用户实测确认)
- [ ] 2.2 单元测试覆盖筛选判定(推迟至 `add-backend-unit-tests` change 统一补齐)

## 附:codex 审阅追加修复(超出原任务、已完成)
- [x] 3.1 `WatchConfig` 手写 `Default` 复用 `default_suffixes`,修复全新安装压制所有裸文件通知
- [x] 3.2 前端 `passesFilter` 后缀比较 `toLowerCase`,与后端大小写不敏感对齐
- [x] 3.3 新增 `get_filter` 命令,前端启动同步持久化筛选,消除重启后前后端分叉
- [x] 3.4 `filterEdited` 守卫,避免启动异步加载覆盖用户即时修改
