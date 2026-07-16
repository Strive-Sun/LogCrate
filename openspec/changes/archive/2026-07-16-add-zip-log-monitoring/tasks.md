## 1. 项目脚手架
- [x] 1.1 初始化 Tauri 2.x 工程(`src-tauri` + 前端 Vite/React/TS)
- [ ] 1.2 配置 `cargo fmt` / `clippy` 与前端 Prettier/lint
  - 已完成:CI 跑 `cargo fmt --all --check` 与 `cargo clippy -D warnings`
  - 待办:前端无 Prettier/ESLint 配置与 lint 脚本,CI 仅有 `tsc --noEmit` + build
- [x] 1.3 确认 Windows + macOS 均能 `tauri dev` 启动空壳(bundle targets=all;CI 矩阵含 windows/macos)
- [x] 1.4 搭建两栏布局骨架(左树 + 右正文,分栏宽度可拖拽并持久化)与 CSS 变量深浅色主题(默认浅色 + 手动切换)
  - 注:按实际实现修正原"三栏 + 跟随系统主题"为"两栏 + 默认浅色手动切换";跟随系统主题不在本 change 范围
- [x] 1.5 引入 `@tanstack/react-virtual`(用于日志正文虚拟滚动)
  - 注:按实际实现修正;Radix 基元未引入,菜单/弹层为手写轻量组件

## 2. 归档读取(archive-reading)
- [x] 2.1 定义 `ArchiveReader` trait(`entries()` / `open_entry() -> Read 流`)
- [ ] 2.2 实现 zip reader(基于 `zip` crate;区分 Stored 可 seek 与 Deflate 顺序流;`entries()` 仅读中央目录不解压)
  - 已完成:zip crate;`entries()` 仅读中央目录
  - 待办:未区分 Stored 可 seek 与 Deflate 顺序流;`open_entry` 目前将整条目读入内存 `Vec<u8>`(非真流式/seek)
- [x] 2.3 实现裸文本 passthrough reader(单文件视为单条目归档,与包内条目共用查看路径)
- [ ] 2.4 实现 `is_log` 判定(扩展名 + 内容采样)
  - 已完成:`is_log_name`(扩展名)+ `is_text_sample`(内容采样),裸文本与 watcher 已用
  - 待办:zip 内条目仅按扩展名判定,未做内容采样
- [ ] 2.5 安全边界:以实际读取字节数熔断大小上限(防 zip bomb);加密/分卷归档返回明确错误
  - 已完成:`open_log_session` 路径按实际字节熔断(2GB `MAX_UNCOMPRESSED`);加密条目返回明确错误
  - 待办:分卷归档无显式错误;`list_archive_entries`/`entries()` 本身无 zip-bomb 熔断
- [x] 2.6 命令 `list_archive_entries(archive_path)`:免解压返回条目列表
- [ ] 2.7 单元测试:列条目仅读清单不解压、打开条目、passthrough、非文本条目标记、声明大小与实际不符时熔断
  - 待办:后端无任何单元测试

## 3. 目录监控(directory-monitoring)
- [x] 3.1 基于 `notify` 实现多目录同时监听(每目录独立 watcher;注:用 `recommended_watcher` 而非已声明的 debouncer)
- [x] 3.2 实现文件到达大小稳定检测(500ms 轮询,连续 3 次稳定)
- [x] 3.3 到达后判定类型:zip 归档(扩展名/magic)或裸文本日志文件(扩展名/内容采样)
- [ ] 3.4 可配置后缀筛选:规则应用于目录树展示与新文件通知,与目录列表一同持久化
  - 已完成:后缀规则随 `WatchConfig` 持久化;前端目录树按 `passesFilter` 过滤
  - 待办:后端检测/通知路径(`classify`/`stable_detect`)未应用用户配置的后缀,仍用硬编码 `is_log_name`,通知会忽略自定义后缀
- [x] 3.5 命令 `add_watch_dir` / `remove_watch_dir` / `list_watch_dirs`
- [x] 3.6 监控目录列表 + 筛选规则持久化到本地配置(JSON);启动时读取并恢复监控
- [x] 3.7 失效目录跳过不阻断启动
- [x] 3.8 目录树惰性展开:展开 zip 节点时才 `list_archive_entries`;裸文本为叶子节点
- [ ] 3.9 单元测试:到达检测逻辑、类型判定、筛选、持久化读写、失效目录跳过
  - 待办:无监控相关单元测试

## 4. 新日志提示(log-notification)
- [x] 4.1 后端在判定为日志包后 emit `new-archive-detected` 事件
- [x] 4.2 前端顶栏组件:接收事件、显示计数与提示
- [ ] 4.3 点击提示 → 展开新日志包列表(计数不变)
  - 已完成:点击铃铛展开新日志列表
  - 待办:点击列表项(压缩包)会立即 `markSeen` 递减计数,未保留"展开时计数不变"的语义
- [x] 4.4 查看某个新包 → 计数减一(已看集合去重,重复查看不递减)
- [x] 4.5 "全部标记已读" → 计数清零
- [ ] 4.6 文件删除/同名覆盖时更新通知列表与计数(监听 remove/覆盖事件)
  - 已完成:前端删除文件时经 `markSeen` 更新列表/计数
  - 待办:无后端 remove/覆盖事件驱动;同名覆盖未单独处理

## 5. 日志查看:行索引 + 窗口化加载(log-viewing)
- [ ] 5.1 命令 `open_log_session`:后台流式扫描条目建行偏移索引,返回 `session_id`;Deflate 条目一趟解压到内部临时缓存(方案 A)后按缓存 seek;解压后 >2GB 拒绝并提示;写盘失败回退为仅顺序读
  - 已完成:建行偏移索引并流式写临时缓存;返回 `session_id`;>2GB 拒绝;按实际字节熔断
  - 待办:当前为同步阻塞(非后台);Deflate 无独立一趟路径(整条目先入内存);无"写盘失败回退顺序读"
- [ ] 5.2 建索引进度通过 `index-progress` 事件反馈;支持边建边读(返回当前已索引行数上界)
  - 待办:后端未 emit `index-progress`;`open` 需全部索引完才返回,不支持真正边建边读
- [x] 5.3 命令 `read_lines(session_id, start, count)`:按偏移读取指定行范围;单行超阈值(64KB)截断并标记
- [ ] 5.4 文本编码检测与解码(UTF-8 / GBK/GB18030 / UTF-16 + BOM),支持手动指定编码
  - 已完成:UTF-8 / UTF-16(BOM)/ GBK 回退检测解码
  - 待办:未用 GB18030;UTF-16BE 被当作 LE;前端 `编码 ▾` 为静态文本,无手动指定编码的命令/重解码
- [x] 5.5 行分隔符兼容 LF/CRLF,返回行去除行尾 `\r`/`\n`
- [ ] 5.6 命令 `close_log_session`:释放行索引与内部临时缓存;会话数超上限时 LRU 回收;进程退出兜底清理所有残留缓存
  - 已完成:`close_log_session` 释放索引 + 临时缓存(Drop);超 `MAX_SESSIONS=5` 时 LRU 回收
  - 待办:进程退出兜底清理(`clear_all`)未挂接到退出钩子,仅靠 Drop
- [x] 5.7 前端虚拟滚动文本视图:按可视区调用 `read_lines`,支持随机跳转
- [ ] 5.8 建索引进度条 UI
  - 已完成:`.index-bar` 进度条组件存在
  - 待办:后端不发进度且索引瞬时返回,进度条实际为死代码
- [ ] 5.9 超长行截断 / 横向虚拟滚动处理
  - 已完成:超长行截断并标记"已截断";`white-space: pre` + 容器 `overflow:auto` 原生横向滚动
  - 待办:为原生横向滚动,非任务描述的横向虚拟化(可评估是否必须)
- [ ] 5.10 单元测试:行偏移索引正确性、边界行读取、CRLF 切分、编码解码、超大单行截断、缓存清理
  - 待办:无任何相关单元测试
- [ ] 5.11 无感体验实测:几百 MB 压缩条目点开首屏 < ~200ms、顺序滚动无掉帧、已就绪范围随机跳转即时(见技术文档 4.4 指标)
  - 待办:需人工实测;`open_entry` 整条目入内存会拖累大条目表现,存在风险

## 6. 集成与验证
- [ ] 6.1 端到端手测:丢 zip 进监控目录 → 顶栏提示 → 点开 → 查看日志(需人工)
- [ ] 6.2 Windows + macOS 双端各跑一遍闭环(CI 覆盖构建/clippy;运行时需人工)
- [ ] 6.3 `openspec validate add-zip-log-monitoring --strict` 通过(需运行 openspec 校验)

## 7. 后续新增能力(超出本 change 原始范围,已实现)
> 以下为 M1 之后按需求追加、已落地的能力;若要纳入规格追踪,建议拆为独立 change 归档。
- [x] 7.1 左树新到达高亮:新文件红色高亮、含新文件的目录琥珀色高亮(前端 `unreadIds` 驱动)
- [x] 7.2 禁用 WebView 默认右键菜单
- [x] 7.3 文件/压缩包右键菜单:在资源管理器打开、重命名(内联)、删除到回收站(带确认)
- [x] 7.4 目录右键菜单:在资源管理器打开、重命名(联动监控配置与监听重建)、移除监控(不删磁盘)、删除目录到回收站
- [x] 7.5 左栏宽度可拖拽调整并持久化(localStorage)
- [x] 7.6 后缀筛选控件移至左栏"监控目录"标题行
