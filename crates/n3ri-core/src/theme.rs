//! 主题包（theme pack）包装器：默认内置 + 外部文件覆盖。
//!
//! 约定（XDG / Linux 哲学）：
//! - 默认 basedir = `~/.config/n3ri_os/`（与 [`UserSettings`](super::config::UserSettings) 同目录）。
//! - `N3RI_CONFIG` 环境变量可整体覆盖 basedir（调试用）；`XDG_CONFIG_HOME` 改变 `~` 解析。
//! - 主题包 = `<basedir>/themes/<name>/` 目录 + `theme.toml` manifest。
//! - 解析顺序永远是路径选取：主题包文件 → 内置 assets → 编译期 embed → 兜底默认。
//!   缺失只回退，不崩。
//!
//! 主题包目录结构：
//! ```text
//! ~/.config/n3ri_os/
//! ├── config.toml              # 只放路径和开关：theme = "my-pack"、enabled_apps = [...]
//! ├── icons/<app>/icon-a.png   # 全局图标覆盖（跨主题生效）
//! ├── fonts/*.woff2            # 全局字体覆盖
//! ├── scripts/*.rhai           # 可选窗口规则脚本（行为钩子，缺失走内置）
//! └── themes/<name>/
//!     ├── theme.toml           # 颜色/字体映射/dock 排序/窗口装饰/背景/live2d
//!     ├── icons/<app>/...      # 本主题图标覆盖
//!     ├── fonts/*.woff2
//!     ├── wallpaper.* / background.wgsl
//!     └── rules.rhai           # 可选：本主题窗口规则
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// 全局配置（`config.toml`）：只放路径和开关，不放颜色细节。
/// 同时是 bevy Resource：设置页读取当前值做提示（只读显示主题来源）。
#[derive(Debug, Clone, Default, Serialize, Deserialize, bevy::prelude::Resource)]
pub struct N3riConfig {
    /// 当前主题名，对应 `themes/<name>/`。`None` = 纯内置默认。
    #[serde(default)]
    pub theme: Option<String>,
    /// 启用的 app id 列表。`None` = 全部内置 app。
    #[serde(default)]
    pub enabled_apps: Option<Vec<String>>,
    /// 用户对主题值的覆盖（设置页写入，优先级高于主题包）。
    #[serde(default)]
    pub overrides: ThemeOverrides,
}

/// 主题包带来的可配字段子集的用户覆盖。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ThemeOverrides {
    #[serde(default)]
    pub colors: HashMap<String, String>,
    #[serde(default)]
    pub fonts: HashMap<String, String>,
}

/// `theme.toml` manifest：声明式为主，行为钩子只给脚本路径。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeManifest {
    #[serde(default = "default_theme_name")]
    pub name: String,
    #[serde(default)]
    pub colors: HashMap<String, String>,
    #[serde(default)]
    pub fonts: ThemeFonts,
    #[serde(default)]
    pub dock: DockTheme,
    #[serde(default)]
    pub window: WindowTheme,
    #[serde(default)]
    pub background: BackgroundTheme,
    #[serde(default)]
    pub live2d: Live2dTheme,
    /// 可选窗口规则脚本（相对主题包根，如 `rules.rhai`）。缺失走内置行为。
    #[serde(default)]
    pub rules_script: Option<String>,
}

fn default_theme_name() -> String {
    "default".into()
}

impl Default for ThemeManifest {
    fn default() -> Self {
        Self {
            name: "default".into(),
            colors: HashMap::new(),
            fonts: ThemeFonts::default(),
            dock: DockTheme::default(),
            window: WindowTheme::default(),
            background: BackgroundTheme::default(),
            live2d: Live2dTheme::default(),
            rules_script: None,
        }
    }
}

/// 字体映射：四路上下文 → 主题包内相对路径（或全局 fonts/ 内）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ThemeFonts {
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default)]
    pub terminal: Option<String>,
    #[serde(default)]
    pub ui: Option<String>,
    #[serde(default)]
    pub dock: Option<String>,
}

/// dock 结构：排序 / 隐藏 / 显示名覆盖。未知 id 直接忽略。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DockTheme {
    #[serde(default)]
    pub order: Vec<String>,
    #[serde(default)]
    pub hidden: Vec<String>,
    #[serde(default)]
    pub display_names: HashMap<String, String>,
}

/// 窗口装饰：纯声明式数字 + 颜色键，不含行为。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowTheme {
    #[serde(default = "default_title_bar_h")]
    pub title_bar_height: f32,
    #[serde(default = "default_corner_radius")]
    pub corner_radius: f32,
    #[serde(default = "default_window_bg")]
    pub window_bg: String,
    #[serde(default = "default_title_bar_bg")]
    pub title_bar_bg: String,
}

fn default_title_bar_h() -> f32 {
    32.0
}
fn default_corner_radius() -> f32 {
    10.0
}
fn default_window_bg() -> String {
    "#141F33F2".into()
}
fn default_title_bar_bg() -> String {
    "#0D1424FA".into()
}

impl Default for WindowTheme {
    fn default() -> Self {
        Self {
            title_bar_height: 32.0,
            corner_radius: 10.0,
            window_bg: "#141F33F2".into(),
            title_bar_bg: "#0D1424FA".into(),
        }
    }
}

/// 背景：壁纸路径 + shader 路径 + shader uniform 参数。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BackgroundTheme {
    /// 壁纸（主题包内相对路径），如 `wallpaper.jpg`。
    #[serde(default)]
    pub wallpaper: Option<String>,
    /// 背景 shader（主题包内相对路径），缺失用内置 `shaders/desktop_background.wgsl`。
    #[serde(default)]
    pub shader: Option<String>,
    /// shader uniform 参数（speed/scale/...），按名透传。
    #[serde(default)]
    pub shader_params: HashMap<String, f32>,
    /// 噪声纹理覆盖（主题包内相对路径）。
    #[serde(default)]
    pub noise: Option<String>,
}

/// Live2D：模型目录选取 + 声明式展示参数。动作逻辑不动。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Live2dTheme {
    /// 模型目录（主题包内相对路径），缺失用内置 `nori/ARGNori_web`。
    #[serde(default)]
    pub model_dir: Option<String>,
    #[serde(default)]
    pub scale: Option<f32>,
    #[serde(default)]
    pub offset: Option<(f32, f32)>,
    #[serde(default)]
    pub rtt_cap: Option<u32>,
}

/// 已解析的主题包：manifest + 各层物理根目录（按优先级）。
/// 同时是 bevy [`Resource`](bevy::prelude::Resource)：`N3riCorePlugin` 启动时解析一次插入。
#[derive(Debug, Clone, Default, bevy::prelude::Resource)]
pub struct ResolvedTheme {
    pub manifest: ThemeManifest,
    /// 搜索序：[主题包根, 全局 basedir, ...]。调用方再 append 内置 assets。
    pub overlay_roots: Vec<PathBuf>,
    /// 当前主题名（`config.toml: theme`）。`None` = 纯内置。
    pub name: Option<String>,
    /// 用户覆盖（`config.toml: [overrides]`），优先级高于主题包。
    pub overrides: ThemeOverrides,
    /// 启用的 app id（`config.toml: enabled_apps`）。`None` = 全部。
    pub enabled_apps: Option<Vec<String>>,
}

/// 从环境解析当前主题：`()` = 读 `config.toml`。
impl From<()> for ResolvedTheme {
    fn from(_: ()) -> Self {
        let cfg = load_config();
        match cfg.theme.clone() {
            Some(name) => {
                let root = theme_dir(&name);
                let manifest = load_theme_manifest(&name).unwrap_or_default();
                Self {
                    manifest,
                    overlay_roots: vec![root, base_dir()],
                    name: Some(name),
                    overrides: cfg.overrides,
                    enabled_apps: cfg.enabled_apps,
                }
            }
            None => Self {
                manifest: ThemeManifest::default(),
                overlay_roots: vec![base_dir()],
                name: None,
                overrides: cfg.overrides,
                enabled_apps: cfg.enabled_apps,
            },
        }
    }
}

impl From<&ResolvedTheme> for N3riConfig {
    fn from(r: &ResolvedTheme) -> Self {
        Self {
            theme: r.name.clone(),
            enabled_apps: r.enabled_apps.clone(),
            overrides: r.overrides.clone(),
        }
    }
}

fn config_base_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("N3RI_CONFIG") {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("n3ri_os")
}

/// 全局 basedir：`~/.config/n3ri_os/`（`N3RI_CONFIG` 可覆盖）。
pub fn base_dir() -> PathBuf {
    config_base_dir()
}

/// 全局图标覆盖目录：`<basedir>/icons/`。
pub fn global_icons_dir() -> PathBuf {
    base_dir().join("icons")
}

/// 全局字体覆盖目录：`<basedir>/fonts/`。
pub fn global_fonts_dir() -> PathBuf {
    base_dir().join("fonts")
}

/// 全局脚本目录：`<basedir>/scripts/`。
pub fn global_scripts_dir() -> PathBuf {
    base_dir().join("scripts")
}

/// 主题包根目录：`<basedir>/themes/<name>/`。
pub fn theme_dir(name: &str) -> PathBuf {
    base_dir().join("themes").join(name)
}

/// 加载全局 `config.toml`。缺失 → 默认（纯内置）。
pub fn load_config() -> N3riConfig {
    let path = base_dir().join("config.toml");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return N3riConfig::default();
    };
    toml::from_str(&text).unwrap_or_default()
}

/// 加载主题包 manifest。缺失/解析失败 → `None`（调用方回退内置默认）。
pub fn load_theme_manifest(name: &str) -> Option<ThemeManifest> {
    let path = theme_dir(name).join("theme.toml");
    let text = std::fs::read_to_string(&path).ok()?;
    toml::from_str(&text).ok()
}

/// 解析当前主题：读 `config.toml` → 找 `themes/<name>/theme.toml`。
/// 返回 manifest（无主题时为 `default()`）+ overlay 搜索序。
pub fn resolve_theme() -> ResolvedTheme {
    ResolvedTheme::from(())
}

/// 在 overlay 根目录中按序查找 assets 相对路径命中的第一个物理文件。
/// 主题包文件 → 全局 basedir →（调用方 append 内置 assets）。
/// 绝对路径直接透传。
pub fn find_overlay_file(rel: &str, overlay_roots: &[PathBuf]) -> Option<PathBuf> {
    let p = Path::new(rel);
    if p.is_absolute() {
        return p.is_file().then(|| p.to_path_buf());
    }
    // 防目录穿越：只允许包内相对路径。
    if rel.contains("..") {
        return None;
    }
    for root in overlay_roots {
        let full = root.join(rel);
        if full.is_file() {
            return Some(full);
        }
    }
    None
}

/// 主题图标查找：`themes/<t>/icons/<app>/...` → 全局 `icons/<app>/...`。
/// 返回首个存在的物理文件绝对路径。
pub fn find_icon_file(app: &str, file: &str, theme: &ResolvedTheme) -> Option<PathBuf> {
    for root in &theme.overlay_roots {
        // 主题包内两种布局都接受：icons/<app>/x 与 nori/app-icons/<app>/x
        for candidate in [
            root.join(format!("icons/{app}/{file}")),
            root.join(format!("nori/app-icons/{app}/{file}")),
        ] {
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// 解析 `#RRGGBB[AA]` / `r,g,b[,a]` 为线性前 sRGB 四元组。失败 → `None`（调用方回退 const）。
pub fn parse_color(s: &str) -> Option<[f32; 4]> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        let alpha = match hex.len() {
            6 => 255,
            8 => u8::from_str_radix(&hex[6..8], 16).ok()?,
            _ => return None,
        };
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        return Some([
            r as f32 / 255.0,
            g as f32 / 255.0,
            b as f32 / 255.0,
            alpha as f32 / 255.0,
        ]);
    }
    let parts: Vec<&str> = s.split(',').collect();
    if parts.len() == 3 || parts.len() == 4 {
        let mut v = [0.0f32; 4];
        v[3] = 1.0;
        for (i, p) in parts.iter().enumerate() {
            v[i] = p.trim().parse::<f32>().ok()?;
            if v[i] > 1.0 {
                v[i] /= 255.0;
            }
        }
        return Some(v);
    }
    None
}
