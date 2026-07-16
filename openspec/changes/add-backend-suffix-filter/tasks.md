## 1. 后端筛选应用
- [ ] 1.1 到达检测回调读取当前持久化的 `suffixes`/`show_all`
- [ ] 1.2 `show_all=false` 时,仅后缀匹配的新文件 emit `new-archive-detected`
- [ ] 1.3 `show_all=true` 时不因后缀过滤通知
- [ ] 1.4 筛选规则变更后对后续到达即时生效

## 2. 验证
- [ ] 2.1 设为仅 `.log` 时,`.tmp`/`.bin` 等新文件不产生通知
- [ ] 2.2 单元测试覆盖筛选判定(与后端测试 change 协同)
