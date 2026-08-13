## ADDED Requirements

### Requirement: DOCX 文档格式识别与图文块流

系统 SHALL 将后缀为 `.docx` 且 OPC 包结构有效、包含 Word 主文档关系与 `word/document.xml` 的文件识别为 DOCX 文档，而不是普通 ZIP 归档。系统 SHALL 将该文档表示为一个可直接打开的文档条目，并在用户打开时以固定缓冲流式解析主文档、输出分页文本/图片块元数据；系统不得把整份 XML、转换后全文或全部图片一次性载入内存/WebView，不得向用户目录释放任何部件。

#### Scenario: 有效 DOCX 作为单个预览条目

- **WHEN** 用户列出结构有效的 `.docx`
- **THEN** 系统返回一个与源文档同名的可查看文档条目，不展示 `[Content_Types].xml`、`word/` 或其它内部 ZIP 条目

#### Scenario: 仅打开时解析正文

- **WHEN** 监控目录发现有效 `.docx` 但用户尚未打开它
- **THEN** 系统只校验格式所需的中央目录和小型包元数据，不解压或扫描 `word/document.xml` 正文

#### Scenario: 流式输出图文块

- **WHEN** 用户打开包含大量正文和多张图片的有效 `.docx`
- **THEN** 系统通过固定缓冲、有界背压和分页块索引边解析边发布 UTF-8 文本与图片元数据，图片二进制保持未读取直至进入预取范围，内存占用不随完整文档线性增长

#### Scenario: 图片关系限定在包内

- **WHEN** 主文档图片关系指向包内普通媒体条目
- **THEN** 系统规范化并校验包内相对路径，通过会话生成的不透明图片 ID 引用该条目，不把包内路径或任意文件读取能力暴露给前端

#### Scenario: 伪造 DOCX 后缀

- **WHEN** `.docx` 文件不是有效 ZIP/OPC 文档，缺少主文档关系或缺少 `word/document.xml`
- **THEN** 系统返回明确的 DOCX 格式无效错误，不回退为普通 ZIP、裸文本或可展开内部条目

#### Scenario: 普通 ZIP 保持原行为

- **WHEN** 有效普通 ZIP 不使用 `.docx` 后缀
- **THEN** 系统继续按归档格式列出其普通文件条目，不因内部存在相似路径而自动改为 DOCX 预览

#### Scenario: DOCX XML 与容器安全边界

- **WHEN** DOCX 损坏、加密、包含禁止的 XML 文档类型或外部实体，或者实际解码、转换输出、扫描时间或其它资源超过统一上限
- **THEN** 系统立即停止解析、清理临时状态并返回对应安全错误，不访问网络、不展开媒体、不崩溃且不继续无界消费资源

### Requirement: DOCX 图片安全读取

系统 SHALL 只向 DOCX 预览提供包内、magic 与 MIME 一致的 PNG/JPEG 图片。图片 SHALL 通过会话内不透明 ID 按需读取，每张实际解码字节不超过 16 MiB、像素不超过 32 MP，前端 Blob 缓存不超过 64 MiB；系统不得信任 ZIP 声明大小或压缩比，外链、路径越界、损坏、超限、EMF/WMF、SVG、GIF 和未知格式 MUST NOT 交给 WebView 解码。

#### Scenario: 惰性读取支持的截图

- **WHEN** PNG/JPEG 图片块进入当前视口预取范围且通过路径、magic、MIME、字节与像素校验
- **THEN** 后端只读取该图片并返回给当前会话，前端生成 Blob URL 显示且不预读其它图片

#### Scenario: 图片缓存有界

- **WHEN** 用户滚动经过的图片 Blob 总量将超过 64 MiB 或关闭 DOCX 会话
- **THEN** 前端按 LRU 撤销不再保留的 Blob URL，关闭时撤销全部 URL，后端释放会话图片索引和文本缓存

#### Scenario: 不支持或危险图片占位

- **WHEN** 图片外链、路径越界、损坏、格式不支持、实际解码超过 16 MiB 或像素超过 32 MP
- **THEN** 后端不返回图片字节，前端在原锚点位置显示含安全短名称和原因的占位，文档其它正文与图片仍可继续预览
