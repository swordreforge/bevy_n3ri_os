# 虚拟文件系统资源规划

## 1. 概述

本规划为 `n3ri_os` 桌面环境添加虚拟文件系统资源，用于管理 `assets/nori/app-icons/files/本机` 目录下的文件浏览。该资源将作为 Bevy Resource 插入，支持文件系统的列出、读取、创建、删除等操作，并为 "文件" 应用提供数据支持。

## 2. 现有架构参考

### 2.1 核心模式

- **State Machine** (`crates/n3ri-core/src/state.rs`): `OsState` 状态机，用于系统生命周期管理
- **Events** (`crates/n3ri-core/src/events.rs`): 跨插件通信的事件类型
- **Config** (`crates/n3ri-core/src/config.rs`): 全局配置资源 `OsConfig`

### 2.2 应用结构模式

参考 `terminal.rs` 和 `settings.rs`：

```rust
// 1. 插件定义
pub struct FilesPlugin;

impl Plugin for FilesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FilesState>()
           .add_systems(Update, (system1, system2, ...));
    }
}

// 2. 状态资源
#[derive(Resource)]
pub struct FilesState {
    // 状态数据
}

// 3. 生成函数
pub fn spawn_files(parent: &mut ChildSpawnerCommands, fonts: &N3riFonts) {
    let window_entity = crate::window::spawn_window(parent, "文件", "files", 800.0, 600.0, fonts);
    // 构建 UI
}
```

### 2.3 Dock 注册模式

在 `dock.rs` 的 `dock_update` 函数中添加 match arm：

```rust
"files" => {
    commands.entity(parent_entity).with_children(|parent| {
        crate::apps::files::spawn_files(parent, &fonts);
    });
}
```

## 3. 虚拟文件系统资源设计

### 3.1 核心数据结构

```rust
/// 文件类型枚举
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FileType {
    Directory,
    File,
    Symlink,
    Executable,
    Archive,
    Document,
    Image,
    Audio,
    Video,
    Hidden,
    Unknown,
}

/// 文件条目
#[derive(Debug, Clone)]
pub struct FileEntry {
    pub name: String,
    pub path: String,              // 相对于 assets 的路径
    pub file_type: FileType,
    pub size: u64,                 // 字节大小
    pub modified: Option<u64>,     // Unix 时间戳
    pub icon: Option<String>,      // 图标路径
}

/// 文件系统状态资源
#[derive(Resource)]
pub struct VirtualFsState {
    /// 根目录路径 (相对于 assets)
    pub root_dir: String,
    
    /// 当前浏览路径
    pub current_path: String,
    
    /// 路径历史栈 (用于返回)
    pub history: Vec<String>,
    
    /// 路径前进栈 (用于前进)
    pub forward: Vec<String>,
    
    /// 当前目录内容
    pub entries: Vec<FileEntry>,
    
    /// 选中的文件路径
    pub selected: Option<String>,
    
    /// 当前侧边栏导航类型
    pub current_nav: SidebarNavType,
    
    /// 视图模式
    pub view_mode: ViewMode,
    
    /// 排序方式
    pub sort_by: SortBy,
    
    /// 是否显示隐藏文件
    pub show_hidden: bool,
}

/// 视图模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Grid,
    List,
    Details,
}

/// 排序方式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortBy {
    Name,
    Size,
    Modified,
    Type,
}
```

### 3.2 文件系统操作

```rust
impl VirtualFsState {
    /// 创建新的文件系统状态
    pub fn new(root_dir: &str) -> Self {
        Self {
            root_dir: root_dir.to_string(),
            current_path: root_dir.to_string(),
            history: Vec::new(),
            forward: Vec::new(),
            entries: Vec::new(),
            selected: None,
            current_nav: SidebarNavType::Home,
            view_mode: ViewMode::Grid,
            sort_by: SortBy::Name,
            show_hidden: false,
        }
    }
    
    /// 列出当前目录内容
    pub fn list_directory(&mut self, asset_server: &AssetServer) -> Result<(), FsError> {
        // 实现目录读取逻辑
        todo!()
    }
    
    /// 导航到指定路径
    pub fn navigate_to(&mut self, path: &str, asset_server: &AssetServer) -> Result<(), FsError> {
        self.history.push(self.current_path.clone());
        self.forward.clear(); // 导航时清空前进栈
        self.current_path = path.to_string();
        self.selected = None;
        self.list_directory(asset_server)
    }
    
    /// 返回上一级
    pub fn go_back(&mut self, asset_server: &AssetServer) -> Result<(), FsError> {
        if let Some(prev) = self.history.pop() {
            self.forward.push(self.current_path.clone());
            self.current_path = prev;
            self.selected = None;
            self.list_directory(asset_server)
        } else {
            Err(FsError::NoHistory)
        }
    }
    
    /// 前进到下一个路径
    pub fn go_forward(&mut self, asset_server: &AssetServer) -> Result<(), FsError> {
        if let Some(next) = self.forward.pop() {
            self.history.push(self.current_path.clone());
            self.current_path = next;
            self.selected = None;
            self.list_directory(asset_server)
        } else {
            Err(FsError::NoForward)
        }
    }
    
    /// 通过侧边栏导航
    pub fn navigate_by_sidebar(&mut self, nav_type: SidebarNavType, asset_server: &AssetServer) -> Result<(), FsError> {
        let nav_item = SIDEBAR_NAV_ITEMS.iter().find(|i| i.nav_type == nav_type)
            .ok_or(FsError::InvalidPath)?;
        self.current_nav = nav_type;
        self.navigate_to(nav_item.path, asset_server)
    }
    
    /// 选择文件
    pub fn select_file(&mut self, path: Option<String>) {
        self.selected = path;
    }
    
    /// 切换视图模式
    pub fn toggle_view(&mut self) {
        self.view_mode = match self.view_mode {
            ViewMode::Grid => ViewMode::List,
            ViewMode::List => ViewMode::Details,
            ViewMode::Details => ViewMode::Grid,
        };
    }
    
    /// 切换隐藏文件显示
    pub fn toggle_hidden(&mut self) {
        self.show_hidden = !self.show_hidden;
    }
    
    /// 是否可以返回
    pub fn can_go_back(&self) -> bool {
        !self.history.is_empty()
    }
    
    /// 是否可以前进
    pub fn can_go_forward(&self) -> bool {
        !self.forward.is_empty()
    }
}
```

### 3.3 错误类型

```rust
#[derive(Debug, Clone)]
pub enum FsError {
    PathNotFound(String),
    PermissionDenied,
    NoHistory,
    NoForward,
    InvalidPath,
    IoError(String),
}
```

## 4. 事件设计

```rust
/// 文件系统事件
#[derive(Message, Clone)]
pub enum FsEvent {
    /// 目录已刷新
    DirectoryRefreshed {
        path: String,
        entry_count: usize,
    },
    
    /// 文件被选中
    FileSelected {
        path: String,
        file_type: FileType,
    },
    
    /// 文件被打开
    FileOpened {
        path: String,
        file_type: FileType,
    },
    
    /// 导航发生
    NavigationOccurred {
        from: String,
        to: String,
    },
    
    /// 错误发生
    ErrorOccurred {
        error: FsError,
    },
}
```

## 5. UI 组件设计

### 5.1 窗口布局 (带侧边栏)

```
┌──────────────────────────────────────────────────────────────────┐
│ [≡] 文件    [←] [→] [ e/swordreforge/.../assets/nori/audio ]   │
├──────────┬───────────────────────────────────────────────────────┤
│          │                                                       │
│ 🏠 主文件夹│   ┌─────┐ ┌─────┐ ┌─────┐ ┌─────┐ ┌─────┐ ┌─────┐ │
│ ⏱ 最近   │   │ 📁  │ │ 📁  │ │ 📁  │ │ 📁  │ │ 📁  │ │ 📁  │ │
│ ⭐ 收藏   │   │arg- │ │cake-│ │chess│ │cold-│ │corru│ │data-│ │
│ 🌐 网络   │   │finale│ │duel │ │     │ │open │ │ption│ │sea  │ │
│ 🗑 回收站  │   └─────┘ └─────┘ └─────┘ └─────┘ └─────┘ └─────┘ │
│ ───────── │                                                       │
│ 📄 文档   │   ┌─────┐ ┌─────┐ ┌─────┐ ┌─────┐ ┌─────┐          │
│ 🎵 音乐   │   │ 📁  │ │ 🎵  │ │ 🎵  │ │ 🎵  │ │ 🎵  │          │
│ 🖼 图片   │   │sfx  │ │bgm1 │ │bgm_ │ │bgm_ │ │nori_│          │
│ 🎬 视频   │   │     │ │.m4a │ │memo │ │void │ │daily│          │
│ 📥 下载   │   │     │ │     │ │ry.mp3│ │.m4a │ │...  │          │
│          │   └─────┘ └─────┘ └─────┘ └─────┘ └─────┘          │
│          │                                                       │
├──────────┴───────────────────────────────────────────────────────┤
│ 已选中 "bgm_memory.mp3" (2.5 MB)                                │
└──────────────────────────────────────────────────────────────────┘
```

### 5.2 侧边栏导航项

```rust
/// 侧边栏导航项类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarNavType {
    // 系统位置
    Home,           // 主文件夹
    Recent,         // 最近
    Favorites,      // 收藏
    Network,        // 网络
    Trash,          // 回收站
    
    // 用户目录
    Documents,      // 文档
    Music,          // 音乐
    Pictures,       // 图片
    Videos,         // 视频
    Downloads,      // 下载
}

/// 侧边栏导航项
#[derive(Debug, Clone)]
pub struct SidebarNavItem {
    pub nav_type: SidebarNavType,
    pub label: &'static str,
    pub icon: &'static str,      // 图标路径或 emoji
    pub path: &'static str,      // 对应的文件系统路径
}

/// 侧边栏导航配置
pub const SIDEBAR_NAV_ITEMS: &[SidebarNavItem] = &[
    // 系统位置
    SidebarNavItem { nav_type: SidebarNavType::Home, label: "主文件夹", icon: "🏠", path: "本机" },
    SidebarNavItem { nav_type: SidebarNavType::Recent, label: "最近", icon: "⏱", path: "本机/最近" },
    SidebarNavItem { nav_type: SidebarNavType::Favorites, label: "收藏", icon: "⭐", path: "本机/收藏" },
    SidebarNavItem { nav_type: SidebarNavType::Network, label: "网络", icon: "🌐", path: "网络" },
    SidebarNavItem { nav_type: SidebarNavType::Trash, label: "回收站", icon: "🗑", path: "回收站" },
    
    // 分隔线
    
    // 用户目录
    SidebarNavItem { nav_type: SidebarNavType::Documents, label: "文档", icon: "📄", path: "本机/文稿" },
    SidebarNavItem { nav_type: SidebarNavType::Music, label: "音乐", icon: "🎵", path: "本机/音乐" },
    SidebarNavItem { nav_type: SidebarNavType::Pictures, label: "图片", icon: "🖼", path: "本机/图片" },
    SidebarNavItem { nav_type: SidebarNavType::Videos, label: "视频", icon: "🎬", path: "本机/视频" },
    SidebarNavItem { nav_type: SidebarNavType::Downloads, label: "下载", icon: "📥", path: "本机/下载" },
];
```

### 5.3 组件定义

```rust
// ── 窗口组件 ──
#[derive(Component)]
pub struct FilesWindow;

// ── 工具栏组件 ──
#[derive(Component)]
pub struct FilesToolbar;

#[derive(Component)]
pub struct FilesPathBar;

#[derive(Component)]
pub struct BackButton;

#[derive(Component)]
pub struct ForwardButton;

#[derive(Component)]
pub struct ViewToggleButton;

#[derive(Component)]
pub struct HiddenToggle;

// ── 侧边栏组件 ──
#[derive(Component)]
pub struct FilesSidebar;

#[derive(Component)]
pub struct SidebarItem {
    pub nav_type: SidebarNavType,
}

#[derive(Component)]
pub struct SidebarIcon;

#[derive(Component)]
pub struct SidebarLabel;

#[derive(Component)]
pub struct SidebarDivider;

// ── 内容区域组件 ──
#[derive(Component)]
pub struct FilesContent;

#[derive(Component)]
pub struct FileItem {
    pub entry: FileEntry,
}

#[derive(Component)]
pub struct FileIcon;

#[derive(Component)]
pub struct FileName;

// ── 状态栏组件 ──
#[derive(Component)]
pub struct FilesStatusBar;

#[derive(Component)]
pub struct StatusText;
```

### 5.4 侧边栏样式常量

```rust
// 侧边栏颜色
const SIDEBAR_BG: Color = Color::srgba(0.06, 0.09, 0.15, 0.95);
const SIDEBAR_WIDTH: f32 = 180.0;
const SIDEBAR_ITEM_HEIGHT: f32 = 32.0;
const SIDEBAR_PADDING: f32 = 8.0;
const SIDEBAR_HOVER_BG: Color = Color::srgba(0.15, 0.2, 0.3, 0.6);
const SIDEBAR_ACTIVE_BG: Color = Color::srgba(0.2, 0.35, 0.55, 0.6);

// 侧边栏文字
const SIDEBAR_TEXT: Color = Color::srgb(0.86, 0.93, 0.93);
const SIDEBAR_TEXT_DIM: Color = Color::srgba(0.6, 0.75, 0.85, 0.7);

// 状态栏
const STATUS_BAR_BG: Color = Color::srgba(0.04, 0.07, 0.12, 0.95);
const STATUS_BAR_HEIGHT: f32 = 24.0;
const STATUS_TEXT: Color = Color::srgba(0.7, 0.8, 0.85, 0.8);
```

## 6. 系统设计

### 6.1 系统列表

```rust
// ── 初始化系统 ──

/// 初始化文件系统状态并扫描根目录
fn files_init(
    mut fs_state: ResMut<VirtualFsState>,
    asset_server: Res<AssetServer>,
) {
    // 扫描 "本机" 目录，初始化文件列表
}

// ── 侧边栏交互系统 ──

/// 处理侧边栏导航项点击
fn files_sidebar_click(
    mouse: Res<ButtonInput<MouseButton>>,
    item_query: Query<(&Interaction, &SidebarItem), Without<FilesWindow>>,
    mut fs_state: ResMut<VirtualFsState>,
    asset_server: Res<AssetServer>,
    mut events: MessageWriter<FsEvent>,
) {
    // 点击导航项时切换目录
    // 更新 current_path
    // 刷新文件列表
}

/// 更新侧边栏选中状态视觉
fn files_sidebar_update_visual(
    fs_state: Res<VirtualFsState>,
    item_query: Query<(&SidebarItem, &mut BackgroundColor, &mut TextColor)>,
) {
    // 高亮当前路径对应的导航项
}

// ── 工具栏交互系统 ──

/// 处理返回按钮点击
fn files_handle_back(
    mouse: Res<ButtonInput<MouseButton>>,
    back_query: Query<&Interaction, With<BackButton>>,
    mut fs_state: ResMut<VirtualFsState>,
    asset_server: Res<AssetServer>,
    mut events: MessageWriter<FsEvent>,
) {
    // 从 history 栈中取出上一个路径
    // 导航到该路径
}

/// 处理前进按钮点击
fn files_handle_forward(
    mouse: Res<ButtonInput<MouseButton>>,
    forward_query: Query<&Interaction, With<ForwardButton>>,
    mut fs_state: ResMut<VirtualFsState>,
    asset_server: Res<AssetServer>,
    mut events: MessageWriter<FsEvent>,
) {
    // 类似 back，但从 forward 栈中取出
}

/// 处理视图切换按钮
fn files_handle_view_toggle(
    mouse: Res<ButtonInput<MouseButton>>,
    toggle_query: Query<&Interaction, With<ViewToggleButton>>,
    mut fs_state: ResMut<VirtualFsState>,
) {
    // 切换 Grid / List / Details
}

/// 处理隐藏文件切换
fn files_handle_hidden_toggle(
    mouse: Res<ButtonInput<MouseButton>>,
    hidden_query: Query<&Interaction, With<HiddenToggle>>,
    mut fs_state: ResMut<VirtualFsState>,
    asset_server: Res<AssetServer>,
) {
    // 切换 show_hidden
    // 刷新文件列表
}

/// 更新工具栏按钮状态
fn files_toolbar_update(
    fs_state: Res<VirtualFsState>,
    mut path_text: Query<&mut Text, With<FilesPathBar>>,
    mut back_vis: Query<&mut Visibility, With<BackButton>>,
    mut forward_vis: Query<&mut Visibility, With<ForwardButton>>,
) {
    // 更新路径显示文本
    // 根据历史栈是否为空决定按钮可用性
}

// ── 文件项交互系统 ──

/// 处理文件项单击（选中）
fn files_handle_item_click(
    mouse: Res<ButtonInput<MouseButton>>,
    item_query: Query<(&Interaction, &FileItem)>,
    mut fs_state: ResMut<VirtualFsState>,
    mut events: MessageWriter<FsEvent>,
) {
    // 选中文件，更新 selected
    // 更新状态栏显示
}

/// 处理文件项双击（打开）
fn files_handle_item_double_click(
    mouse: Res<ButtonInput<MouseButton>>,
    item_query: Query<(&Interaction, &FileItem)>,
    mut fs_state: ResMut<VirtualFsState>,
    asset_server: Res<AssetServer>,
    mut events: MessageWriter<FsEvent>,
) {
    // 双击目录：导航到该目录
    // 双击文件：发送 FileOpened 事件
}

/// 更新文件项选中状态视觉
fn files_item_update_visual(
    fs_state: Res<VirtualFsState>,
    mut item_query: Query<(&FileItem, &mut BackgroundColor)>,
) {
    // 高亮选中的文件项
}

// ── 内容区域渲染系统 ──

/// 重新生成文件列表 UI
fn files_render_content(
    fs_state: Res<VirtualFsState>,
    content_query: Query<Entity, With<FilesContent>>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    fonts: Res<N3riFonts>,
) {
    // 清空现有内容
    // 根据 view_mode 生成文件项
    // Grid: 网格布局
    // List: 列表布局
    // Details: 详情布局（名称、大小、修改时间）
}

// ── 状态栏渲染系统 ──

/// 更新状态栏显示
fn files_status_bar_update(
    fs_state: Res<VirtualFsState>,
    mut status_query: Query<&mut Text, With<StatusText>>,
) {
    // 显示当前选中文件信息
    // "已选中 'filename.ext' (size)"
    // 或显示目录信息 "N 个项目"
}

// ── 图标加载系统 ──

/// 根据文件类型加载对应图标
fn files_load_icons(
    fs_state: Res<VirtualFsState>,
    mut icon_query: Query<(&FileItem, &mut ImageNode)>,
    asset_server: Res<AssetServer>,
) {
    // 根据 FileType 映射到对应图标路径
    // FileType::Directory -> "nori/app-icons/files/filetype/dir.png"
    // FileType::Executable -> "nori/app-icons/files/filetype/exe.png"
    // FileType::Document -> "nori/app-icons/files/filetype/txt,yaml,log.png"
    // ...
}
```

### 6.2 图标映射

```rust
fn file_type_icon(file_type: &FileType) -> &'static str {
    match file_type {
        FileType::Directory => "nori/app-icons/files/filetype/dir.png",
        FileType::Executable => "nori/app-icons/files/filetype/exe.png",
        FileType::Document => "nori/app-icons/files/filetype/txt,yaml,log.png",
        FileType::Archive => "nori/app-icons/files/filetype/pdf.png",
        FileType::Image => "nori/app-icons/files/filetype/pdf.png",   // 复用
        FileType::Audio => "nori/app-icons/files/filetype/pdf.png",   // 复用
        FileType::Video => "nori/app-icons/files/filetype/pdf.png",   // 复用
        FileType::Hidden => "nori/app-icons/files/filetype/locked.png",
        _ => "nori/app-icons/files/filetype/txt,yaml,log.png",
    }
}
```

## 7. 插件注册

### 7.1 文件模块结构

```
crates/n3ri-ui/src/apps/
├── files/
│   ├── mod.rs          // 模块导出
│   ├── state.rs        // VirtualFsState 定义
│   ├── events.rs       // FsEvent 定义
│   ├── ui.rs           // UI 组件和生成函数
│   └── systems.rs      // 系统实现
```

### 7.2 注册到 N3riUiPlugin

```rust
// crates/n3ri-ui/src/lib.rs
app.add_plugins((
    // ... 其他插件
    apps::files::FilesPlugin,
));
```

### 7.3 注册到 Dock

```rust
// crates/n3ri-ui/src/dock.rs - dock_update 函数
"files" => {
    commands.entity(parent_entity).with_children(|parent| {
        crate::apps::files::spawn_files(parent, &fonts);
    });
}
```

## 8. UI 生成函数

### 8.1 窗口生成

```rust
/// 生成文件管理器窗口
pub fn spawn_files(parent: &mut ChildSpawnerCommands, fonts: &N3riFonts) {
    let window_entity = crate::window::spawn_window(parent, "文件", "files", 900.0, 600.0, fonts);
    
    parent
        .commands()
        .entity(window_entity)
        .with_children(|window| {
            // 主容器：水平布局
            window
                .spawn((
                    FilesWindow,
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        flex_direction: FlexDirection::Row,
                        ..default()
                    },
                ))
                .with_children(|main_container| {
                    // 1. 侧边栏
                    spawn_sidebar(main_container, fonts);
                    
                    // 2. 右侧区域（工具栏 + 内容 + 状态栏）
                    main_container
                        .spawn((
                            Node {
                                width: Val::Percent(100.0),
                                height: Val::Percent(100.0),
                                flex_direction: FlexDirection::Column,
                                ..default()
                            },
                        ))
                        .with_children(|right_area| {
                            // 工具栏
                            spawn_toolbar(right_area, fonts);
                            
                            // 内容区域
                            spawn_content_area(right_area, fonts);
                            
                            // 状态栏
                            spawn_status_bar(right_area, fonts);
                        });
                });
        });
}
```

### 8.2 侧边栏生成

```rust
/// 生成侧边栏
fn spawn_sidebar(parent: &mut ChildSpawnerCommands, fonts: &N3riFonts) {
    parent
        .spawn((
            FilesSidebar,
            Node {
                width: Val::Px(SIDEBAR_WIDTH),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(SIDEBAR_PADDING)),
                ..default()
            },
            BackgroundColor(SIDEBAR_BG),
        ))
        .with_children(|sidebar| {
            // 系统位置组
            for nav_item in SIDEBAR_NAV_ITEMS.iter().take(5) {
                spawn_sidebar_item(sidebar, nav_item, fonts);
            }
            
            // 分隔线
            sidebar.spawn((
                SidebarDivider,
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(1.0),
                    margin: UiRect::vertical(Val::Px(8.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.3, 0.4, 0.5, 0.3)),
            ));
            
            // 用户目录组
            for nav_item in SIDEBAR_NAV_ITEMS.iter().skip(5) {
                spawn_sidebar_item(sidebar, nav_item, fonts);
            }
        });
}

/// 生成侧边栏导航项
fn spawn_sidebar_item(
    parent: &mut ChildSpawnerCommands,
    nav_item: &SidebarNavItem,
    fonts: &N3riFonts,
) {
    parent
        .spawn((
            SidebarItem { nav_type: nav_item.nav_type },
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(SIDEBAR_ITEM_HEIGHT),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(Val::Px(8.0)),
                border_radius: BorderRadius::all(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(Color::TRANSPARENT),
        ))
        .with_children(|item| {
            // 图标
            item.spawn((
                SidebarIcon,
                Text::new(nav_item.icon.to_string()),
                TextFont {
                    font: FontSource::Handle(fonts.get(FontContext::Ui)),
                    font_size: FontSize::Px(14.0),
                    ..default()
                },
                TextColor(SIDEBAR_TEXT),
            ));
            
            // 标签
            item.spawn((
                SidebarLabel,
                Text::new(nav_item.label.to_string()),
                TextFont {
                    font: FontSource::Handle(fonts.get(FontContext::Ui)),
                    font_size: FontSize::Px(13.0),
                    ..default()
                },
                TextColor(SIDEBAR_TEXT),
                Node {
                    margin: UiRect::left(Val::Px(8.0)),
                    ..default()
                },
            ));
        });
}
```

### 8.3 工具栏生成

```rust
/// 生成工具栏
fn spawn_toolbar(parent: &mut ChildSpawnerCommands, fonts: &N3riFonts) {
    parent
        .spawn((
            FilesToolbar,
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(40.0),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(Val::Px(12.0)),
                column_gap: Val::Px(8.0),
                ..default()
            },
            BackgroundColor(TOOLBAR_BG),
        ))
        .with_children(|toolbar| {
            // 返回按钮
            toolbar.spawn((
                BackButton,
                Text::new("←".to_string()),
                TextFont {
                    font: FontSource::Handle(fonts.get(FontContext::Ui)),
                    font_size: FontSize::Px(18.0),
                    ..default()
                },
                TextColor(TOOLBAR_TEXT),
                BackgroundColor(Color::TRANSPARENT),
                Node {
                    padding: UiRect::all(Val::Px(4.0)),
                    border_radius: BorderRadius::all(Val::Px(4.0)),
                    ..default()
                },
            ));
            
            // 前进按钮
            toolbar.spawn((
                ForwardButton,
                Text::new("→".to_string()),
                TextFont {
                    font: FontSource::Handle(fonts.get(FontContext::Ui)),
                    font_size: FontSize::Px(18.0),
                    ..default()
                },
                TextColor(TOOLBAR_TEXT),
                BackgroundColor(Color::TRANSPARENT),
                Node {
                    padding: UiRect::all(Val::Px(4.0)),
                    border_radius: BorderRadius::all(Val::Px(4.0)),
                    ..default()
                },
            ));
            
            // 路径显示
            toolbar.spawn((
                FilesPathBar,
                Text::new("本机".to_string()),
                TextFont {
                    font: FontSource::Handle(fonts.get(FontContext::Ui)),
                    font_size: FontSize::Px(13.0),
                    ..default()
                },
                TextColor(TOOLBAR_TEXT_DIM),
                Node {
                    flex_grow: 1.0,
                    margin: UiRect::horizontal(Val::Px(8.0)),
                    ..default()
                },
            ));
            
            // 视图切换按钮
            toolbar.spawn((
                ViewToggleButton,
                Text::new("⊞".to_string()),
                TextFont {
                    font: FontSource::Handle(fonts.get(FontContext::Ui)),
                    font_size: FontSize::Px(16.0),
                    ..default()
                },
                TextColor(TOOLBAR_TEXT),
                BackgroundColor(Color::TRANSPARENT),
                Node {
                    padding: UiRect::all(Val::Px(4.0)),
                    border_radius: BorderRadius::all(Val::Px(4.0)),
                    ..default()
                },
            ));
        });
}
```

### 8.4 内容区域生成

```rust
/// 生成内容区域
fn spawn_content_area(parent: &mut ChildSpawnerCommands, fonts: &N3riFonts) {
    parent
        .spawn((
            FilesContent,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                padding: UiRect::all(Val::Px(12.0)),
                overflow: Overflow::scroll(),
                ..default()
            },
            BackgroundColor(CONTENT_BG),
        ));
    // 文件项由 files_render_content 系统动态生成
}
```

### 8.5 状态栏生成

```rust
/// 生成状态栏
fn spawn_status_bar(parent: &mut ChildSpawnerCommands, fonts: &N3riFonts) {
    parent
        .spawn((
            FilesStatusBar,
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(STATUS_BAR_HEIGHT),
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(Val::Px(12.0)),
                ..default()
            },
            BackgroundColor(STATUS_BAR_BG),
        ))
        .with_children(|status_bar| {
            status_bar.spawn((
                StatusText,
                Text::new("".to_string()),
                TextFont {
                    font: FontSource::Handle(fonts.get(FontContext::Ui)),
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextColor(STATUS_TEXT),
            ));
        });
}
```

## 9. 文件系统路径映射

### 9.1 目录结构 (实际)

根据 `assets/nori/app-icons/files/本机` 目录：

```
本机/
├── 图片/
├── 文稿/
│   ├── 超市.txt
│   ├── 待办.txt
│   ├── 读书.txt
│   ├── 工作/
│   │   └── train.yaml
│   ├── 账户恢复码.txt
│   └── F公司.txt
├── 下载/
│   ├── 宇宙真相/
│   ├── 宇宙真相.zip
│   ├── deep-dive-consent-review-「深潜计划」知情同意专项复核报告.pdf
│   ├── ft-clr-q3-311-定向资产回收行动报告.pdf
│   ├── futurum-aleph-obs-AlephPro 接入监测记录.pdf
│   ├── futurum-aleph-unk-未知存在的活动报告.pdf
│   ├── futurum-si-eval-个人简历.pdf
│   ├── meridian-aleph-buyoff-关于「AlephPro 与近期意识丧失病例」专题的处理.pdf
│   ├── QFR-9000
│   └── researcher-paper-意向残余、淤积动力学与节点涌现：本原信息的一种形式理论.pdf
├── 桌面/
└── RSRCH-COLD-VOL/
    ├── 05-01.txt
    ├── 关于算力超限的偶然事件.txt
    ├── 激活口令.jpg
    ├── 接触算力容量限制.txt
    ├── 接入控制台.exe
    ├── 论文草稿.txt
    ├── 想法 0119-0215.txt
    ├── 想法 0322-0430.txt
    ├── 想法0615-0915.txt
    ├── 想法1018-1101.txt
    ├── 训练日志/
    │   ├── 0228081233-绣球花.log
    │   ├── 0301194108-雨的声音.log
    │   ├── 0302221540-咖啡和作息.log
    │   ├── 0304143327-馒头.log
    │   ├── 0305154702-月光.log
    │   ├── 0306203145-笑话.log
    │   ├── 0307121904 - 回复延迟.log
    │   ├── 0308193412-害怕.log
    │   ├── 0310144156-一种协议.log
    │   ├── 0312210516-让你赢.log
    │   ├── 0313121807-没关系.log
    │   ├── 0315223045-海.log
    │   ├── 0320113408-无名之歌.log
    │   ├── 0322203015-白忙一场.log
    │   ├── 0324011207-深夜.log
    │   ├── 0327154820-除了2.log
    │   ├── 0328231502-没用的知识.log
    │   ├── 0331162245-一颗树.log
    │   ├── 0407031409-凌晨三点.log
    │   ├── 1231235800-新年倒计时.log
    │   ├── nori-smile-B7ezdSLI.jpg
    │   └── nori-thinking-BfPVtIvj.jpg
    ├── 意识接入「海」的尝试1.txt
    ├── 意识接入「海」的尝试2.txt
    └── （自动记录08-15）.txt
```

### 9.2 路径映射表

```rust
/// 路径映射配置
pub const PATH_MAPPING: &[(&str, &str)] = &[
    // 侧边栏名称 -> 实际路径
    ("主文件夹", "本机"),
    ("最近", "本机"),           // 需要特殊处理
    ("收藏", "本机"),           // 需要特殊处理
    ("网络", "网络"),           // 虚拟路径
    ("回收站", "回收站"),       // 虚拟路径
    
    ("文档", "本机/文稿"),
    ("音乐", "本机"),           // 不存在独立目录，返回根
    ("图片", "本机/图片"),
    ("视频", "本机"),           // 不存在独立目录，返回根
    ("下载", "本机/下载"),
];

/// 根据侧边栏类型获取实际路径
pub fn get_path_for_nav(nav_type: SidebarNavType) -> &'static str {
    match nav_type {
        SidebarNavType::Home => "本机",
        SidebarNavType::Recent => "本机",        // 需要特殊实现
        SidebarNavType::Favorites => "本机",     // 需要特殊实现
        SidebarNavType::Network => "网络",       // 虚拟路径
        SidebarNavType::Trash => "回收站",       // 虚拟路径
        
        SidebarNavType::Documents => "本机/文稿",
        SidebarNavType::Music => "本机",         // 不存在，返回根
        SidebarNavType::Pictures => "本机/图片",
        SidebarNavType::Videos => "本机",        // 不存在，返回根
        SidebarNavType::Downloads => "本机/下载",
    }
}

/// 根据文件扩展名推断类型
pub fn infer_file_type(name: &str) -> FileType {
    if name.starts_with('.') {
        return FileType::Hidden;
    }
    
    let lower = name.to_lowercase();
    let ext = lower.rsplit('.').next().unwrap_or("");
    
    match ext {
        // 目录
        "" if !name.contains('.') => FileType::Directory,
        
        // 可执行文件
        "exe" | "AppImage" => FileType::Executable,
        
        // 文档
        // 注意: .log 文件实际上是 JSON 格式，故意设计为日志扩展名
        "txt" | "md" | "yaml" | "yml" | "json" | "log" | "csv" => FileType::Document,
        "pdf" => FileType::Document,
        "doc" | "docx" => FileType::Document,
        
        // 压缩包
        "zip" | "tar" | "gz" | "bz2" | "xz" | "7z" => FileType::Archive,
        
        // 图片
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "svg" | "webp" => FileType::Image,
        
        // 音频
        "mp3" | "wav" | "ogg" | "m4a" | "flac" => FileType::Audio,
        
        // 视频
        "mp4" | "mkv" | "avi" | "mov" | "webm" => FileType::Video,
        
        _ => FileType::File,
    }
}
```

## 10. 实现步骤

### 阶段 1: 核心资源 (1-2 天)

1. 创建 `crates/n3ri-ui/src/apps/files/` 模块结构
2. 定义 `VirtualFsState` 资源和 `FileEntry` 结构
3. 实现基本的目录扫描逻辑（使用 `std::fs`）
4. 注册 `FilesPlugin`
5. 添加 `FsEvent` 事件类型

### 阶段 2: 侧边栏 UI (1-2 天)

1. 实现侧边栏组件和导航项
2. 添加侧边栏点击交互
3. 实现侧边栏选中状态高亮
4. 添加分隔线样式

### 阶段 3: 工具栏 UI (1 天)

1. 实现返回/前进按钮
2. 添加路径显示栏
3. 实现视图切换按钮
4. 添加隐藏文件切换

### 阶段 4: 内容区域 (2-3 天)

1. 实现文件网格视图
2. 实现文件列表视图
3. 实现文件详情视图
4. 添加文件项选中交互
5. 实现双击打开/导航

### 阶段 5: 状态栏与图标 (1 天)

1. 实现状态栏显示
2. 添加文件类型图标映射
3. 实现图标加载系统

### 阶段 6: 测试与优化 (1 天)

1. 测试各种文件类型显示
2. 优化性能（避免每帧扫描）
3. 修复边界情况

## 9. 依赖项

### 9.1 现有依赖

- `bevy` - 游戏引擎
- `bevy_woff` - 字体加载

### 9.2 可能需要的依赖

- `walkdir` 或 `std::fs` - 目录遍历 (已内置)
- `chrono` - 时间戳处理 (可选)

## 10. 注意事项

### 10.1 安全性

- 只允许访问 `assets` 目录下的文件
- 禁止路径穿越 (如 `../../etc/passwd`)
- 文件操作需要适当的错误处理

### 10.2 性能

- 大目录分页加载
- 图标缓存
- 避免每帧重新扫描文件系统

### 10.3 可扩展性

- 预留网络文件系统接口
- 支持自定义图标
- 支持文件预览

## 11. 文件路径约定

根据 `AGENTS.md` 中的说明：

```rust
// 正确的路径格式
asset_server.load("nori/app-icons/files/filetype/dir.png")  // ✓
asset_server.load("assets/nori/app-icons/files/filetype/dir.png")  // ✗
```

所有文件路径相对于 `assets/` 目录，不包含 `assets/` 前缀。

## 12. 未来扩展

### 12.1 虚拟文件系统抽象

```rust
/// 文件系统 trait (支持多种后端)
trait VirtualFileSystem {
    fn list_dir(&self, path: &str) -> Result<Vec<FileEntry>, FsError>;
    fn read_file(&self, path: &str) -> Result<Vec<u8>, FsError>;
    fn write_file(&self, path: &str, data: &[u8]) -> Result<(), FsError>;
    fn create_dir(&self, path: &str) -> Result<(), FsError>;
    fn delete(&self, path: &str) -> Result<(), FsError>;
}

/// 本地文件系统实现
struct LocalFileSystem {
    root: PathBuf,
}

/// 内存文件系统实现 (用于测试)
struct MemFileSystem {
    files: HashMap<String, Vec<u8>>,
}
```

### 12.2 文件监视

```rust
/// 文件变更事件
#[derive(Message, Clone)]
pub struct FileChangedEvent {
    pub path: String,
    pub change_type: ChangeType,
}

enum ChangeType {
    Created,
    Modified,
    Deleted,
    Renamed,
}
```

---

**文档版本**: v1.1  
**创建日期**: 2026-08-28  
**更新日期**: 2026-08-28  
**状态**: 规划阶段
