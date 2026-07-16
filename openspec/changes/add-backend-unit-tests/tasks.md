## 1. archive-reading 测试
- [ ] 1.1 `list_archive_entries` 仅读中央目录、不产生解压产物
- [ ] 1.2 `open_entry` 打开 Stored 与 Deflate 条目内容正确
- [ ] 1.3 裸文本 passthrough:单文件视为单条目
- [ ] 1.4 非文本条目被标记 `is_log=false`
- [ ] 1.5 声明大小与实际读取不符/超上限时熔断报错

## 2. directory-monitoring 测试
- [ ] 2.1 `classify` 类型判定:zip magic、裸文本扩展名/采样
- [ ] 2.2 后缀筛选规则应用与边界
- [ ] 2.3 配置持久化写入与读取回环(含 suffixes/show_all)
- [ ] 2.4 失效目录跳过不 panic

## 3. log-viewing 测试
- [ ] 3.1 行偏移索引正确性(含空行、末行无换行)
- [ ] 3.2 边界行读取(start/count 越界、尾行)
- [ ] 3.3 LF 与 CRLF 切分,行尾 `\r`/`\n` 去除
- [ ] 3.4 编码解码:UTF-8、GBK、含 BOM 的 UTF-16
- [ ] 3.5 超大单行(> 64KB)截断并标记
- [ ] 3.6 `close_log_session` 释放缓存;LRU 回收后临时文件被删除

## 4. CI 集成
- [ ] 4.1 确认 CI 执行 `cargo test`(Windows + macOS 矩阵)
- [ ] 4.2 修复测试暴露的缺陷(如有)
