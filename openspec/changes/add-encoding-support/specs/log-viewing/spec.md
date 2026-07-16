## MODIFIED Requirements
### Requirement: 文本编码检测与解码
系统 SHALL 检测日志条目的文本编码并解码为可读文本,至少支持 UTF-8、GBK/GB18030、UTF-16(含 BOM 识别,正确区分 LE 与 BE),避免非 UTF-8 日志出现乱码或读取失败。系统 SHALL 提供命令按用户指定编码对当前会话重新解码,前端 SHALL 提供可交互的编码选择控件展示当前编码并允许手动覆盖自动检测结果。

#### Scenario: 自动识别并解码非 UTF-8 日志
- **WHEN** 日志条目内容采用 GBK/GB18030 或含 BOM 的 UTF-16(LE 或 BE)编码
- **THEN** 系统检测其编码并正确解码为可读文本返回,不出现乱码或因非 UTF-8 字节而报错

#### Scenario: 区分 UTF-16 字节序
- **WHEN** 日志条目为含 BOM 的 UTF-16BE
- **THEN** 系统按 BE 解码(不被误当作 LE),输出正确文本

#### Scenario: 手动指定编码
- **WHEN** 自动检测结果不正确,用户在编码控件中手动选择目标编码
- **THEN** 系统按用户指定编码重新解码并刷新当前视图
