# 归档依赖评估

## 结论

| 格式 | 选择 | 许可证 | 原生依赖 | Rust 1.70 | 结论 |
|---|---|---|---|---|---|
| TAR | `tar 0.4.41` | MIT / Apache-2.0 | 无 | 是 | 采用 |
| gzip | `flate2 1.0.30` | MIT / Apache-2.0 | 默认 Rust 后端 | 是 | 采用 |
| bzip2 | `bzip2 0.4.4` | MIT / Apache-2.0 | 构建时编译 libbz2 | 是 | 采用 |
| xz | `xz2 0.1.7` | MIT / Apache-2.0 | 构建时编译 liblzma | 是 | 采用 |
| zstd | `zstd 0.13.1` | MIT | 构建时编译 zstd | 是 | 采用 |
| 7z | `sevenz-rust 0.6.1` | Apache-2.0 | 无 | 明确声明 1.70 | 采用；关闭压缩功能与 AES，启用 bzip2/zstd 解码 |
| RAR | `unrar_sys 0.5.8` | bindings 为 MIT / Apache-2.0；内嵌 UnRAR 为专用许可 | 静态编译 UnRAR C++ | Windows 已验证；macOS 待 CI | 采用底层 callback；正式发布仍需双平台与包体积验收 |

## 未选择统一 libarchive 后端

`compress-tools 0.16.1` 要求 Rust 1.82，超过项目声明的 Rust 1.70；Windows MSVC 还要求 vcpkg，macOS 通常要求 Homebrew/pkg-config。动态或静态再分发、updater 安装后的加载路径和包体积都明显复杂于格式专用后端，因此本 change 不采用。

## 7z

`sevenz-rust 0.6.1` 是纯 Rust Apache-2.0 实现，支持 COPY、LZMA、LZMA2，并可选解码 bzip2、zstd。读取接口可以先读取元数据，并在 `for_each_entries` 回调中将目标条目写入有界通道；solid 归档会按格式要求解码目标之前的数据，但不会把这些内容积累在内存或释放到用户目录。

首版不启用 AES feature。头部或内容要求密码时统一返回“归档已加密，暂不支持密码输入”；未知算法、校验失败和损坏分别保留底层诊断。

## RAR 发布门槛

`rars 0.4.4` 虽为纯 Rust MIT / Apache-2.0，并声明覆盖早期 RAR 到 RAR 7，但要求 Rust 1.87，且维护者仍将成熟度描述为 “works. ish.”，不符合当前 MSRV 与发布质量要求。

`unrar 0.5.8` 能识别 RAR4/RAR5、加密和分卷，但高层 `read()` 会返回完整 `Vec<u8>`，`extract_to()` 又无法按实际解码字节及时中止，均不满足 GB 日志与恶意归档的安全边界。因此实现直接使用 `unrar_sys 0.5.8`：以 `RAR_TEST` 解码，不创建目标文件；`UCM_PROCESSDATA` 回调按最多 64 KiB 分块写入容量为 2 的有界通道，并在接收端取消、密码请求、换卷请求或实际字节超过 2 GiB 时返回 `-1` 中止。

此外，UnRAR 原生源码使用专用许可证；正式启用前必须完成以下事项：

- 由项目维护者确认二进制再分发条款和许可证随包要求；
- 在 Windows 与 macOS CI 验证静态 C++ 构建、RAR4/RAR5 单卷夹具；
- 实现 callback 级实际解码字节限制和取消，不使用整条目 `Vec`；
- 明确拒绝加密和分卷，并验证不会自动查找相邻卷；
- 记录安装包体积增量、安全公告来源和依赖升级负责人。

当前 Windows 开发构建已用真实 RAR4 内容条目和 RAR5 Unicode 头夹具验证枚举与流式读取，完整 UnRAR 许可证保存在 `resources/licenses/unrar.txt`。macOS CI、安装包体积和 updater 安装验证仍是正式发布门槛。
