# 主题包（theme pack）包装器

默认内置 + 外部文件覆盖。永远是**路径选取**：主题包文件 → 全局
`~/.config/n3ri_os/` → 内置 `assets/` → 编译期 embed → 兜底默认。
缺失只回退，不崩。完整字段示例见 `theme-pack-example.toml`。

## 目录约定（XDG）

```text
~/.config/n3ri_os/
├── config.toml              # 只放路径和开关：theme / enabled_apps / [overrides.*]
├── icons/<app>/icon-a.png   # 全局图标覆盖（跨主题生效）
├── fonts/*.woff2            # 全局字体覆盖
├── scripts/*.rhai           # 预留：窗口规则脚本
└── themes/<name>/
    ├── theme.toml           # 颜色/字体/dock/窗口装饰/背景/live2d
    ├── icons/<app>/...      # 本主题图标覆盖（另接受 icons/<app>/ 短式布局）
    ├── fonts/*.woff2
    └── models/my-pet/       # 可选 live2d 模型目录（须含 .model3.json）
```

`N3RI_CONFIG` 环境变量可整体覆盖 basedir（调试用）。
`config.toml` 示例：

```toml
theme = "demo"
enabled_apps = ["terminal", "files", "browser"]

[overrides.colors]
text = "#FF0000"   # 设置页写入，用户覆盖，优先级高于主题包
```

## 可配范围（本版已接线）

- 基础图标：dock + 顶栏状态图标（`theme://` 命中则换，否则内置）。
- 字体：`[fonts]` 四路（default/terminal/ui/dock）。
- 颜色：`[colors]` 九键 → `ThemeConfig`（`N3riCorePlugin` 启动时应用一次）。
- dock 结构：`[dock] order/hidden/display_names` + 顶层 `enabled_apps` 裁剪。
  未知 id 忽略；`settings` 常驻末尾（除非 hidden/未启用）。
- 窗口装饰：`[window]` 标题栏高/圆角/两色，spawn 时读一次。
- 背景：`[background.noise]` 噪声纹理 + `shader_params.speed` 时间流速。
- Live2D：`[live2d] model_dir` 主题模型目录（失败 warn 后回退内置）。
- 虚拟内容（files/mail/signal 文本层）：`content::read_bytes` 先查主题包。

## 预留未接线（TOML 位已留，行为仍走内置）

- `[background] wallpaper/shader`：自定义壁纸图与 WGSL 热替换。
- `rules_script` / `scripts/*.rhai`：窗口行为钩子（on_focus/on_resize/on_snap）。
  行为脚本化建议用 `rhai`（纯 Rust、无 FFI），缺失脚本即走内置默认。

## 实现位置

- `crates/n3ri-core/src/theme.rs`：basedir/`config.toml`/`theme.toml` 解析、
  `ResolvedTheme` resource、`find_overlay_file`/`find_icon_file`/`parse_color`。
- `crates/n3ri-ui/src/theme_source.rs`：`register_theme_sources`（注册
  `theme://` + `theme-global://` 双源，`main.rs` 窗口/壁纸两路已调用）与
  `theme_asset_path` 显式路由。
- 接线点：`font.rs` / `dock.rs` / `window.rs`（`window_decor()`）/ `topbar.rs` /
  `desktop.rs`（noise+speed）/ `content.rs` / `n3ri-live2d/loader.rs`
  （`theme_model_dir()`，不依赖 n3ri-core，直读环境）。
- 不配：布局 px（磁吸/间距/z-order）、点击拖拽滚动物理、Servo UA/首页。

## 验证

```bash
cargo check --workspace --all-targets
```
