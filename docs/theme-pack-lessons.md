# 主题包经验教训（theme pack lessons）

这次把 `~/.config/n3ri_os/themes/<name>/` 做成"默认内置 + 外部覆盖"包装器时，
连续踩了 8 个坑（全是静默回退，看起来"没一点用"的那种）。逐条记录，
下次碰 AssetSource / 外部资源先看这篇。

## L1：命名 AssetSource 必须在 `DefaultPlugins` 之前注册

- 现象：`AssetSourceId::Name(theme) must be registered before AssetPlugin`，
  接着 `Source 'theme' does not exist`。
- 根因：`register_asset_source` 内部硬门——`AssetServer` resource 一旦存在直接
  报错拒绝。而 `DefaultPlugins` 整组 `add_plugins` 时 `AssetPlugin::build`
  同步就把 `AssetServer` 建好了。任何放在 `DefaultPlugins` 之后的时间点
  （哪怕紧贴下一行）都注定失败。
- 修复：`register_theme_sources(app)` 函数改为 `ThemeSourcePlugin`，
  `main.rs` 窗口/壁纸两路都在 `EmbeddedAssetPlugin` 之后、`DefaultPlugins`
  之前 `add_plugins(ThemeSourcePlugin)`。
  顺序：embed（占 Default）→ theme 命名源 → DefaultPlugins。
- 规则：凡是 `register_asset_source`，一律做成 Plugin 并排在 DefaultPlugins 前。
  不要试图在运行时"补注册"。

## L2：`override_unapproved` 在默认 `Forbid` 下无效

- 现象：`Asset path /home/.../texture_00.png is unapproved`，宠物只剩白色轮廓。
- 根因：Bevy 源码（`server/mod.rs`）门禁是
  `(Allow, _) | (Deny, true) => {}` —— `Forbid` 下无论有没有 override 都拒。
  `override_unapproved()` 只在 `Deny` 下有效，默认 `Forbid` 下是摆设。
- 修复：放弃"loader 侧直读绝对路径"，改为把主题纹理**镜像拷贝**进主题包内
  `nori/live2d-theme/<模型目录名>/...`，走已注册的 `theme://` File 源。
  loader 照样按扩展名分发（ktx2/png 通吃）。
- 规则：默认构建永远假设 `Forbid`。外部绝对路径想进 AssetServer，
  唯一正路是"拷进某个已注册源的根之下"，不要指望 override 开后门。

## L3：mirror 的"根"拿错，路径越拼越怪

- 现象：`Path not found: .../themes/demo/nori/live2d-theme/<hash>/Nori.4096/texture_00.png`。
- 根因：`mirror_theme_texture(theme_root, rel)` 里传进去的 `theme_root`
  实际是**模型目录**（`.../demo/Nori_web`），函数内又 `theme_root.join(rel)`，
  等于把模型目录当成了主题包根，源文件和目标镜像全错位。
- 修复：`Live2dPet.texture_base_override` 从存"模型绝对目录"改为存
  `(主题包根, 模型目录名)` —— `load_pet` 里 `theme_dir.parent()` 即包根，
  `file_name()` 即 `Nori_web`（= theme.toml `model_dir` 原值，直接复用）。
- 规则：凡是"根 + 相对路径"二元组，字段名必须写清是谁的根。
  存绝对路径一时爽，拼镜像时必错位。

## L4：镜像路径别用哈希，用人类可读名

- 现象：`live2d-theme/b4a45a0d6fdb2afd/...` —— 报错时人肉对不上是哪只模型。
- 根因：早期用 `DefaultHasher(theme_root)` 做隔离 tag，调试时无法反查。
- 修复：镜像路径改为 `nori/live2d-theme/<模型目录名>/<rel>`，
  如 `nori/live2d-theme/Nori_web/Nori.4096/texture_00.png`。
  隔离靠目录名本身（不同模型名天然不同），可读性优先。
- 规则：落盘的中间路径一律人类可读。哈希只用在真正需要去重/定长的地方。

## L5：手写 TOML 行解析会吞行尾注释

- 现象：`model_dir = "Nori_web"  # 中文注释`，目录存在却报不存在，静默回退内置。
- 根因：`n3ri-live2d` 为避开对 `n3ri-core` 的依赖而手写行解析，
  只去首尾引号，值变成 `Nori_web"  # ...`。
- 修复：加 `toml_string_value()` ——引号外 `#` 截断、引号内 `#` 保留
  （颜色值 `"#112233"` 不误伤）。`config.toml` 的 `theme` 解析同病一起换。
  另加回归测试 `toml_trailing_comment_is_stripped`。
- 规则：手写解析必须有注释测试用例。或者干脆引 `toml` crate
  （`n3ri-core` 侧就是这么做的，`live2d` 侧为了零依赖才手写，要写就写全）。

## L6：loader 和 renderer 的纹理前缀必须同源

- 现象（历史）：`load_pet` 主题分支只换了 moc/motion 读取目录，
  纹理 `renderer.rs` 还拼内置 `MODEL_DIR` 前缀 → 主题 model3 指向
  `tex/a.png`，却去内置 assets 里找 → 404 整宠失败。
- 修复：`Live2dPet` 加 `texture_base_override` 字段，renderer 命中主题时
  改走 mirror + `theme://`（见 L2/L3）。
- 规则：模型加载是"两段式"（loader 读 CPU 数据 + renderer 读 GPU 纹理），
  换源必须两段一起换，只换一段必出"一半主题一半内置"的幽灵 bug。

## L7：主题模型必须完整自洽，缺一件整包回退

- 内容：`validate_theme_model_dir` —— model3.json 可解析 + moc 存在非空 +
  `Textures` 数组每一张存在 + 至少一组 motion 首文件存在。
  缺任何一项 → `theme_model_dir()` 返回 None，`load_pet` 走内置。
  `..` 一律拒绝。坏文件内容（corrupt png/坏 moc）是第二道门，
  `load_pet_from_dir` Err → warn 后回退。
- 为什么：不允许"只覆盖单张纹理"——model3 与纹理混用必然白模/花屏，
  且用户会误以为"生效了"。要么整只换，要么别换。
- 规则：外部包校验永远"存在性在前（进分支前），内容级在后（分支内 Err 回退）"。

## L8：`embed-assets` 构建会动 Default 源，别碰它

- 内容：`EmbeddedAssetPlugin::ReplaceDefault` 在 AssetPlugin 之前先占
  Default builder（`AssetPlugin::init_default_source` 是 get_or_insert，
  不再覆盖）。命名源（`theme://`）不受影响，但注册必须走正式 API
  `App::register_asset_source`，不要直接写 `AssetSourceBuilders` 内部结构。
- 规则：只用公开 API（`register_asset_source` + `AssetPath::with_source`），
  不碰 `AssetSources` 内部。`--features embed-assets` 和普通构建双双 check
  才算过。

## L9：冒烟测试必须自包含

- 内容：`crates/n3ri-live2d/tests/theme_validate_smoke.rs` ——现场从仓库
  `assets/nori/ARGNori_web` 拷真 moc3/motion，在 `temp_dir` 组装临时主题目录，
  覆盖 ok / 缺 motion / 缺纹理 / 行尾注释四种。不依赖外部 fixture，
  `cargo test -p n3ri-live2d` 直接跑（4 passed）。
- 规则：外部路径相关的测试，fixture 现造现删，不吃仓库常驻文件，
  不吃开发者本机 `~/.config`。

## 落地检查清单（下次加新主题资源类型时照着走）

1. 新源？→ Plugin 化，排 DefaultPlugins 前，双构建 check。
2. 外部绝对路径？→ 镜像进已注册源根下，不指望 override。
3. 路径二元组？→ 字段名写清是谁的根，落盘路径人类可读。
4. 手写解析？→ 注释用例先行。
5. 加载分两段？→ 两段同源切换。
6. 外部包？→ 存在性校验进分支前，内容失败分支内回退，缺件整包退。
7. 测试？→ temp_dir 自包含，`cargo test -p <crate>` 一键跑。
