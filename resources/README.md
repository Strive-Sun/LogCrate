# Resources

项目的非代码资源统一存放在此目录，并按用途分类：

- `icons/app/`：LogCrate SVG 图标母版，以及由母版生成的桌面平台 PNG、ICO 和 ICNS。
- `screenshots/`：README 与发布文档使用的产品截图。

重新生成应用图标时，以 `icons/app/logcrate.svg` 为唯一母版，并将输出目录指定为 `resources/icons/app`，不要重新创建 `src-tauri/icons`。

README 主界面截图约定：

- `screenshots/logcrate-hero-light.png`：浅色主题。
- `screenshots/logcrate-hero-dark.png`：深色主题。
- 两张图片必须保持相同尺寸，单张不超过 1 MiB，且不得包含真实用户名、绝对路径、IP 或敏感日志。
