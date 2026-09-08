use bevy::input::keyboard::KeyboardInput;
use bevy::input::mouse::MouseWheel;
use bevy::log::DEFAULT_FILTER;
use bevy::prelude::*;
use bevy::ui::IsDefaultUiCamera;
use bevy::window::PrimaryWindow;
use bevy_framepace::{FramepacePlugin, FramepaceSettings, Limiter};
use bevy_live_wallpaper::{LiveWallpaperCamera, LiveWallpaperPlugin};
use n3ri_core::prelude::*;
use n3ri_live2d::{
    HeadDisplay, Live2dPet, PetDisplayNode, PetRenderConfig, PetTargetArea,
    spawn_head_display, spawn_pet_display, HeadDisplayWanted, PetDisplayImage,
    PetHeadImage,
};
use n3ri_ui::cursor::{CursorPosition, UiArea};
use n3ri_ui::desktop::DesktopBackgroundMaterial;
use n3ri_ui::font::N3riFonts;
use n3ri_ui::wallpaper_bridge::{
    SatelliteDeltaChannel, SatelliteFrame, WallpaperInputBridgePlugin,
};
use n3ri_ui::wallpaper_ime::WallpaperImePlugin;
use n3ri_ui::wallpaper_keyboard::WallpaperKeyboardPlugin;
use n3ri_ui::window::AppWindow;
use n3ri_ui::N3riUiPlugin;
use n3ri_ui::chat_capsule::{ChatEmotionEvent, ChatRise};
use std::io::{Read, Write};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::ConnectionExt;
use x11rb::protocol::Event as X11Event;

mod focus;
mod music_player;

use music_player::MusicPlayerPlugin;

const BG_DARK: Color = Color::srgb(0.02, 0.05, 0.1);
const CYAN: Color = Color::srgba(0.4, 0.95, 1.0, 1.0);
const TRACK_COLOR: Color = Color::srgba(0.15, 0.2, 0.25, 0.5);

/// debug 构建保留默认 info 级日志；release 只留 error，最大化省 I/O
/// （Servo/webrender 的 info/warn 噪声占大头）。运行时可用 RUST_LOG 覆盖：
/// `RUST_LOG=info cargo run --release -p n3ri-minimal`
fn log_filter() -> String {
    if cfg!(debug_assertions) {
        format!("{},icu_provider=off", DEFAULT_FILTER)
    } else {
        "error,icu_provider=off".to_string()
    }
}

/// 画质档位 → 宠物 RTT 最长边上限（像素）。
/// 极限性能 1280（38fps）/ 平衡 1920（33fps，默认）/ 省电 960（最低 GPU 负载）
fn pet_rtt_cap(quality_idx: usize) -> u32 {
    match quality_idx {
        0 => 1280,
        1 => 1920,
        _ => 960,
    }
}

/// 启动时按 UserSettings.quality_idx 初始化宠物渲染配置（启动前 N3riCorePlugin
/// 已 load() 完 JSON；Startup 之后由 sync_pet_render_config 持续跟随设置页改动）
fn init_pet_render_config(settings: Res<UserSettings>, mut config: ResMut<PetRenderConfig>) {
    config.rtt_cap = pet_rtt_cap(settings.quality_idx);
}

/// 设置页改画质 → 实时写回渲染配置（refit_pet_view 防抖后自动缩小/放大 RTT）
fn sync_pet_render_config(settings: Res<UserSettings>, mut config: ResMut<PetRenderConfig>) {
    let cap = pet_rtt_cap(settings.quality_idx);
    if config.rtt_cap != cap {
        config.rtt_cap = cap;
    }
}

/// fps_idx → bevy_framepace 限帧器。0(无限制) = Limiter::Off，维持 AutoNoVsync 原帧行为。
fn limiter_for_fps_idx(idx: usize) -> Limiter {
    match n3ri_core::config::fps_limit_hz(idx) {
        Some(hz) => Limiter::from_framerate(hz),
        None => Limiter::Off,
    }
}

fn same_limiter(a: &Limiter, b: &Limiter) -> bool {
    match (a, b) {
        (Limiter::Off, Limiter::Off) | (Limiter::Auto, Limiter::Auto) => true,
        (Limiter::Manual(a), Limiter::Manual(b)) => a == b,
        _ => false,
    }
}

/// 设置页「帧率上限」→ UserSettings.fps_idx → 写 FramepaceSettings。
/// FramepacePlugin 的 update_proxy_resources 每帧把该值同步进 render 子世界
/// （RenderSystems::Cleanup 的 spin_sleep），下一帧即生效，无需重启。
/// 仅窗口模式注册本系统（壁纸模式无 FramepacePlugin，走 wallpaper_frame_pace 的 wait 换算）。
fn sync_fps_limiter(settings: Res<UserSettings>, mut fp: ResMut<FramepaceSettings>) {
    let limiter = limiter_for_fps_idx(settings.fps_idx);
    if !same_limiter(&fp.limiter, &limiter) {
        fp.limiter = limiter;
    }
}

/// vsync 档 → 主窗 present mode：开 = AutoVsync（自动垂直同步），关 = AutoNoVsync。
fn present_mode_for(vsync: bool) -> bevy::window::PresentMode {
    if vsync {
        bevy::window::PresentMode::AutoVsync
    } else {
        bevy::window::PresentMode::AutoNoVsync
    }
}

/// 设置页「垂直同步」→ Window.present_mode。bevy_render 侦测 present_mode 变化
/// 即 configure_surface 重建（AutoVsync→FifoRelaxed/Fifo 回退链），下一帧生效。
/// 仅窗口模式注册（壁纸模式无主窗）。
fn sync_vsync(
    settings: Res<UserSettings>,
    mut windows: Query<&mut bevy::window::Window, With<bevy::window::PrimaryWindow>>,
) {
    let target = present_mode_for(settings.vsync);
    for mut window in windows.iter_mut() {
        if window.present_mode != target {
            window.present_mode = target;
        }
    }
}

/// 设置页「抗锯齿」→ 主相机 Msaa 组件（per-camera MSAA，bevy 每帧按组件重建附件）。
/// 只在设置变更后写——启动期尊重 N3RI_MSAA 环境变量 A/B（spawn 相机已按 env/设置初始化）。
fn sync_msaa(settings: Res<UserSettings>, mut cameras: Query<&mut Msaa, With<UiMainCamera>>) {
    if !settings.is_changed() {
        return;
    }
    let target = msaa_for_idx(settings.msaa_idx);
    for mut msaa in cameras.iter_mut() {
        if *msaa != target {
            *msaa = target;
        }
    }
}

/// 启动时把 `UserSettings.wallpaper_enabled` 对齐到真实启动模式（CLI `--wallpaper`）。
/// 设置页「显示效果 → 壁纸模式」开关只读该字段；直接用 CLI 进壁纸模式时文件仍是
/// 旧值会导致开关显示为关闭。N3riCorePlugin 在之后 load()，读到的即已对齐的值。
fn sync_wallpaper_setting(actual_wallpaper: bool) {
    let mut settings = UserSettings::load();
    if settings.wallpaper_enabled != actual_wallpaper {
        settings.wallpaper_enabled = actual_wallpaper;
        settings.save();
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print_help();
        return;
    }
    if args.iter().any(|a| a == "--satellite") {
        std::process::exit(run_satellite());
    }
    if args.iter().any(|a| a == "--wallpaper") {
        sync_wallpaper_setting(true);
        run_wallpaper();
    } else {
        sync_wallpaper_setting(false);
        run_windowed();
    }
}

fn print_help() {
    println!("n3ri_os");
    println!("  (无参数)      窗口模式（伪 OS 桌面主窗）");
    println!("  --wallpaper   壁纸模式（layer-shell 桌面壁纸，全 UI 进壁纸层；键盘/IME 直连，滚轮经 vendored 捕获）");
    println!("  --satellite   全局指针卫星进程（XQueryPointer 轮询 → stdout 绝对坐标流；壁纸模式自动拉起）");
    println!("  -h, --help    显示本帮助");
}

fn run_windowed() {
    let mut app = App::new();
    app.insert_resource(n3ri_agent::WallpaperMode(false));

    // ReplaceDefault 必须先于 AssetPlugin 注册（之后添加会 panic），资源路径调用点零改动
    #[cfg(feature = "embed-assets")]
    app.add_plugins(bevy_embedded_assets::EmbeddedAssetPlugin {
        mode: bevy_embedded_assets::PluginMode::ReplaceDefault,
    });

    app.add_plugins(
        DefaultPlugins
            .set(bevy::log::LogPlugin {
                filter: log_filter(),
                ..default()
            })
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "n3ri_os".into(),
                    resolution: (1920u32, 1080u32).into(),
                    // 关闭垂直同步：低端机上 Fifo 会把 ~30ms 的帧时间钳到 30fps、
                    // ~50ms 钳到 20fps（按 vblank 倍数取整）；AutoNoVsync 让
                    // 帧就绪即呈现（Immediate→Mailbox 回退），帧率跟实际帧时间走。
                    // 不支持该模式的平台回退 Fifo，无副作用。
                    present_mode: bevy::window::PresentMode::AutoNoVsync,
                    ..default()
                }),
                ..default()
            })
            .set(AssetPlugin {
                file_path: "../../assets".into(),
                ..default()
            }),
    )
        .add_plugins(N3riCorePlugin::default())
        .add_plugins(N3riUiPlugin)
        .add_plugins(n3ri_agent::AgentPlugin)
        .add_plugins(MusicPlayerPlugin)
        .add_plugins(n3ri_live2d::N3riLive2dPlugin)
        .add_plugins(FramepacePlugin)
        .add_plugins(focus::FocusPlugin);
    // N3RI_PROF=1：控制台每 2s 打印进程 CPU/内存占用与真实帧率（fps/frame_time）。
    // 注：0.19 的 SystemInformationDiagnosticsPlugin 只统计系统/进程级利用率，
    // 并不测量单个 ECS 系统耗时——逐系统归因请用 `perf top` 或 tracy。
    #[cfg(feature = "profiling")]
    if std::env::var("N3RI_PROF").is_ok() {
        use bevy::diagnostic::{
            FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin, SystemInformationDiagnosticsPlugin,
        };
        app.add_plugins((
            SystemInformationDiagnosticsPlugin,
            FrameTimeDiagnosticsPlugin::default(),
            LogDiagnosticsPlugin {
                wait_duration: std::time::Duration::from_secs(2),
                ..Default::default()
            },
        ));
    }
    app.add_systems(Startup, spawn_camera)
        .add_systems(Startup, init_pet_render_config)
        .add_systems(Startup, sync_fps_limiter)
        .add_systems(Update, (chat_rise_sync, chat_emotion_bridge))
        .add_systems(Update, sync_pet_render_config)
        .add_systems(Update, sync_fps_limiter)
        .add_systems(Update, (sync_vsync, sync_msaa))
        .add_systems(OnEnter(OsState::Boot), spawn_boot_screen)
        .add_systems(
            Update,
            update_boot_screen.run_if(in_state(OsState::Boot)),
        )
        .add_systems(OnEnter(OsState::Loading), spawn_loading_screen)
        .add_systems(
            Update,
            update_loading_screen.run_if(in_state(OsState::Loading)),
        )
        .add_systems(OnEnter(OsState::Desktop), spawn_desktop_screen)
        .add_systems(OnEnter(OsState::Desktop), mark_agent_session)
        .add_systems(Update, (toggle_head_display,))
        .run();
}

fn mark_agent_session(time: Res<Time>, mut sched: ResMut<n3ri_agent::SchedulerState>) {
    n3ri_agent::mark_session_start(&mut sched, time.elapsed_secs_f64());
}

fn run_wallpaper() {
    let mut app = App::new();
    app.insert_resource(n3ri_agent::WallpaperMode(true));

    #[cfg(feature = "embed-assets")]
    app.add_plugins(bevy_embedded_assets::EmbeddedAssetPlugin {
        mode: bevy_embedded_assets::PluginMode::ReplaceDefault,
    });

    app.add_plugins(
        DefaultPlugins
            .set(bevy::log::LogPlugin {
                filter: log_filter(),
                ..default()
            })
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                ..default()
            })
            .set(AssetPlugin {
                file_path: "../../assets".into(),
                ..default()
            }),
    )
        .add_plugins(N3riCorePlugin::default())
        .add_plugins(N3riUiPlugin)
        .add_plugins(n3ri_agent::AgentPlugin)
        .add_plugins(MusicPlayerPlugin)
        .add_plugins(n3ri_live2d::N3riLive2dPlugin)
        .add_plugins(focus::FocusPlugin)
        .add_plugins(LiveWallpaperPlugin::default())
        .add_plugins(WallpaperInputBridgePlugin)
        .add_plugins(WallpaperImePlugin)
        .add_plugins(WallpaperKeyboardPlugin)
        // 无主窗口时 winit 判定"未聚焦"走 reactive_low_power，整应用掉到 ~8fps：
        // 按键释放延迟一帧以上。Continuous 在无窗口下不触发重绘（应用冻结），
        // Reactive+wait 是唯一既有节奏又持续 tick 的模式，15ms ≈ 66fps 上限。
        .insert_resource(bevy::winit::WinitSettings {
            focused_mode: bevy::winit::UpdateMode::Reactive {
                wait: std::time::Duration::from_millis(15),
                react_to_device_events: true,
                react_to_user_events: true,
                react_to_window_events: true,
            },
            unfocused_mode: bevy::winit::UpdateMode::Reactive {
                wait: std::time::Duration::from_millis(15),
                react_to_device_events: true,
                react_to_user_events: true,
                react_to_window_events: true,
            },
        });
    // N3RI_PROF=1：控制台每 2s 打印进程 CPU/内存占用与真实帧率（fps/frame_time）。
    // 注：0.19 的 SystemInformationDiagnosticsPlugin 只统计系统/进程级利用率，
    // 并不测量单个 ECS 系统耗时——逐系统归因请用 `perf top` 或 tracy。
    #[cfg(feature = "profiling")]
    if std::env::var("N3RI_PROF").is_ok() {
        use bevy::diagnostic::{
            FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin, SystemInformationDiagnosticsPlugin,
        };
        app.add_plugins((
            SystemInformationDiagnosticsPlugin,
            FrameTimeDiagnosticsPlugin::default(),
            LogDiagnosticsPlugin {
                wait_duration: std::time::Duration::from_secs(2),
                ..Default::default()
            },
        ));
    }
    app.add_systems(Startup, (spawn_wallpaper_camera, spawn_satellite_process))
        .add_systems(Startup, init_pet_render_config)
        .init_resource::<WallpaperFramePace>()
        .add_systems(
            Update,
            (
                chat_rise_sync,
                chat_emotion_bridge,
                sync_pet_target_area,
                track_satellite_child,
                sync_pet_render_config,
                wallpaper_frame_pace,
                sync_msaa,
            ),
        )
        .add_systems(OnEnter(OsState::Boot), spawn_boot_screen)
        .add_systems(
            Update,
            update_boot_screen.run_if(in_state(OsState::Boot)),
        )
        .add_systems(OnEnter(OsState::Loading), spawn_loading_screen)
        .add_systems(
            Update,
            update_loading_screen.run_if(in_state(OsState::Loading)),
        )
        .add_systems(OnEnter(OsState::Desktop), spawn_desktop_screen)
        .add_systems(OnEnter(OsState::Desktop), mark_agent_session)
        .add_systems(Update, (toggle_head_display,))
        .run();
}

/// 设置页「抗锯齿」档位 → bevy Msaa（1 sample = Off）。
fn msaa_for_idx(idx: usize) -> Msaa {
    Msaa::from_samples(n3ri_core::config::msaa_samples(idx))
}

/// 主相机（UI + 桌面背景 shader）MSAA：默认取 UserSettings.msaa_idx
/// （设置 → 显示效果 → 抗锯齿）；`N3RI_MSAA=0|1|2|4|8` 环境变量优先
/// （低端机 A/B，Live2D RTT 同款见 n3ri-live2d renderer）。0/1 = Off（1 sample）。
fn camera_msaa(settings: &UserSettings) -> Msaa {
    match std::env::var("N3RI_MSAA").ok().as_deref() {
        Some("0") | Some("1") => Msaa::Off,
        Some("2") => Msaa::Sample2,
        Some("4") => Msaa::Sample4,
        Some("8") => Msaa::Sample8,
        _ => msaa_for_idx(settings.msaa_idx),
    }
}

/// 受「抗锯齿」档位驱动的屏上相机（窗口模式主相机 / 壁纸模式 surface 相机）。
#[derive(Component)]
struct UiMainCamera;

fn spawn_camera(mut commands: Commands, settings: Res<UserSettings>) {
    commands.spawn((Camera2d, UiMainCamera, camera_msaa(&settings)));
}

/// 统一 UiArea → 宠物视口目标（物理像素 = 逻辑 × scale）。
/// 窗口模式 scale 取自主窗（niri 扩窗即触发 refit）；壁纸模式 scale=1（与壁纸 surface 逻辑链一致）。
fn sync_pet_target_area(
    area: Res<UiArea>,
    cursor: Res<CursorPosition>,
    mut target: ResMut<PetTargetArea>,
) {
    if area.x > 1.0 && area.y > 1.0 {
        let next = PetTargetArea {
            logical: area.0,
            scale: cursor.scale.max(1.0),
        };
        if target.logical != next.logical || target.scale != next.scale {
            *target = next;
        }
    }
}

fn spawn_wallpaper_camera(mut commands: Commands, settings: Res<UserSettings>) {
    commands.spawn((
        Camera2d,
        LiveWallpaperCamera,
        IsDefaultUiCamera,
        UiMainCamera,
        camera_msaa(&settings),
    ));
}

/// 壁纸模式静置降频：无输入 3 秒后 wait 15ms→33ms（66fps→30fps），
/// 追平与窗口模式的差距并减半 page fault 建页开销。任一输入即恢复。
/// 只在 Desktop 降频，Boot/Loading 保持全速。壁纸模式无主窗，
/// focused/unfocused 两档必须一起改（focused 判定不可靠）。
/// 「帧率上限」档位在此生效：活动 wait = 1000/hz（24→42ms、30→33ms…），
/// 无限制维持 15ms；静置档 = 活动档与 33ms 的下限取大（≤30fps 档不再降频）。
/// 壁纸模式不用 bevy_framepace——reactive wait 与 render cleanup spin_sleep 是
/// 两个叠加节流器，混用会让实际帧周期 = limit + wait（例：24fps 档变 ~18fps）。
const WALLPAPER_ACTIVE_WAIT_MS: u64 = 15;
const WALLPAPER_IDLE_WAIT_MS: u64 = 33;
const WALLPAPER_IDLE_TIMEOUT_SECS: f32 = 3.0;

#[derive(Resource, Default)]
struct WallpaperFramePace {
    last_active_secs: f32,
    downclocked: bool,
}

/// fps_idx → 壁纸模式 winit Reactive wait（毫秒）。
fn wallpaper_wait_ms(user: &UserSettings, active: bool) -> u64 {
    let active_ms = match n3ri_core::config::fps_limit_hz(user.fps_idx) {
        Some(hz) => (1000.0 / hz).round().clamp(1.0, 1000.0) as u64,
        None => WALLPAPER_ACTIVE_WAIT_MS,
    };
    if active {
        active_ms
    } else {
        active_ms.max(WALLPAPER_IDLE_WAIT_MS)
    }
}

#[allow(clippy::too_many_arguments)]
fn wallpaper_frame_pace(
    time: Res<Time>,
    state: Res<State<OsState>>,
    cursor: Res<CursorPosition>,
    frame: Res<SatelliteFrame>,
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: MessageReader<KeyboardInput>,
    ime: MessageReader<Ime>,
    wheel: MessageReader<MouseWheel>,
    user: Res<UserSettings>,
    mut winit_settings: ResMut<bevy::winit::WinitSettings>,
    mut pace: ResMut<WallpaperFramePace>,
) {
    fn set_wait(settings: &mut bevy::winit::WinitSettings, ms: u64) {
        let wait = std::time::Duration::from_millis(ms);
        for mode in [
            &mut settings.focused_mode,
            &mut settings.unfocused_mode,
        ] {
            if let bevy::winit::UpdateMode::Reactive {
                wait: ref mut w, ..
            } = mode
            {
                *w = wait;
            }
        }
    }

    let active_wait = wallpaper_wait_ms(&user, true);
    let idle_wait = wallpaper_wait_ms(&user, false);

    let now = time.elapsed_secs();
    // 设置页切换「帧率上限」：change tick 独立于输入，直接落新 wait（保持当前活动/静置档），
    // 不必等下一次输入才生效。
    if user.is_changed() {
        set_wait(
            &mut winit_settings,
            if pace.downclocked { idle_wait } else { active_wait },
        );
    }
    // MessageReader 是广播语义：is_empty 只读游标不消费，各消费方互不干扰。
    let active = cursor.is_changed()
        || frame.scroll != Vec2::ZERO
        || mouse.just_pressed(MouseButton::Left)
        || mouse.just_pressed(MouseButton::Right)
        || mouse.just_pressed(MouseButton::Middle)
        || mouse.just_released(MouseButton::Left)
        || mouse.just_released(MouseButton::Right)
        || mouse.just_released(MouseButton::Middle)
        || !keyboard.is_empty()
        || !ime.is_empty()
        || !wheel.is_empty();
    if active {
        pace.last_active_secs = now;
        if pace.downclocked {
            set_wait(&mut winit_settings, active_wait);
            pace.downclocked = false;
        }
        return;
    }
    if *state.get() == OsState::Desktop
        && !pace.downclocked
        && now - pace.last_active_secs > WALLPAPER_IDLE_TIMEOUT_SECS
    {
        set_wait(&mut winit_settings, idle_wait);
        pace.downclocked = true;
    }
}

#[derive(Resource)]
struct SatelliteChild(std::process::Child);

fn spawn_satellite_process(mut commands: Commands) {
    let Ok(exe) = std::env::current_exe() else {
        warn!("无法定位自身可执行文件，卫星进程未启动");
        return;
    };
    let result = std::process::Command::new(exe)
        .arg("--satellite")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn();
    let mut child = match result {
        Ok(child) => child,
        Err(e) => {
            warn!("卫星进程启动失败: {e}");
            return;
        }
    };

    let Some(stdout) = child.stdout.take() else {
        commands.insert_resource(SatelliteChild(child));
        return;
    };
    commands.insert_resource(SatelliteChild(child));

    let (tx, rx) = std::sync::mpsc::channel::<n3ri_ui::wallpaper_bridge::SatelliteSample>();
    std::thread::spawn(move || {
        let mut reader = std::io::BufReader::new(stdout);
        let mut line = String::new();
        loop {
            line.clear();
            match std::io::BufRead::read_line(&mut reader, &mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let mut parts = line.split_whitespace();
                    let sample = match parts.next() {
                        Some("w") => match (
                            parts.next().and_then(|v| v.parse::<f32>().ok()),
                            parts.next().and_then(|v| v.parse::<f32>().ok()),
                        ) {
                            (Some(dx), Some(dy)) => {
                                Some(
                                    n3ri_ui::wallpaper_bridge::SatelliteSample::Scroll(Vec2::new(dx, dy)),
                                )
                            }
                            _ => None,
                        },
                        Some("s") => match (
                            parts.next().and_then(|v| v.parse::<f32>().ok()),
                            parts.next().and_then(|v| v.parse::<f32>().ok()),
                        ) {
                            (Some(w), Some(h)) => Some(
                                n3ri_ui::wallpaper_bridge::SatelliteSample::Screen(Vec2::new(w, h)),
                            ),
                            _ => None,
                        },
                        Some(_) => match (
                            parts.next().and_then(|v| v.parse::<f32>().ok()),
                            parts.next().and_then(|v| v.parse::<f32>().ok()),
                        ) {
                            (Some(x), Some(y)) => {
                                Some(n3ri_ui::wallpaper_bridge::SatelliteSample::Pos(Vec2::new(x, y)))
                            }
                            _ => None,
                        },
                        None => None,
                    };
                    if let Some(sample) = sample {
                        let _ = tx.send(sample);
                    }
                }
            }
        }
    });
    commands.insert_resource(SatelliteDeltaChannel(std::sync::Mutex::new(rx)));
}

fn track_satellite_child(mut child: ResMut<SatelliteChild>, mut logged: Local<bool>) {
    if *logged {
        return;
    }
    match child.0.try_wait() {
        Ok(Some(status)) => {
            warn!("卫星进程已退出: {status}（全局指针外推不可用，视差将被冻结）");
            *logged = true;
        }
        Ok(None) => {}
        Err(e) => {
            warn!("卫星进程状态查询失败: {e}");
            *logged = true;
        }
    }
}

fn run_satellite() -> i32 {
    // 父进程退出 → stdin EOF → 卫星自杀，避免孤儿进程
    std::thread::spawn(|| {
        let mut buf = String::new();
        let _ = std::io::stdin().read_to_string(&mut buf);
        std::process::exit(0);
    });

    let (conn, screen) = match x11rb::connect(None) {
        Ok(conn) => conn,
        Err(e) => {
            eprintln!("卫星进程连接 Xwayland 失败: {e}");
            return 1;
        }
    };
    let root = conn.setup().roots[screen].root;

    // 核心协议按钮事件：在根窗上选中 BUTTON_PRESS/BUTTON_RELEASE（全局捕获，指针被遮挡时
    // 依然有效）。滚轮在 X11 中是按钮 4/5/6/7 的 press/release 对。失败仅降级为无滚轮，指针流不受影响。
    let wheel_ok = setup_wheel_capture(&conn, root);

    // 首行上报 X 屏物理尺寸（= 逻辑 × 合成器缩放），壁纸侧据此推算真实缩放：
    // 光标坐标换算与 pet RTT 分辨率都依赖它
    let stdout = std::sync::Mutex::new(std::io::stdout());
    {
        let w = conn.setup().roots[screen].width_in_pixels;
        let h = conn.setup().roots[screen].height_in_pixels;
        if let Ok(mut out) = stdout.lock() {
            let _ = writeln!(out, "s {w} {h}");
            let _ = out.flush();
        }
    }

    // 120Hz 轮询 XQueryPointer：读合成器最终光标（触摸板/鼠标通吃、绝对坐标零漂移、
    // 指针被其他窗口遮挡时依然有效）；位置变化才发行，避免无谓的行流
    let mut last: (i16, i16) = (i16::MIN, i16::MIN);
    let mut wheel: (f32, f32) = (0.0, 0.0);
    loop {
        if wheel_ok {
            // 排干按钮事件：滚轮按钮 4/5/6/7 累积为 dx/dy（press/release 成对，
            // 只计 press 避免翻倍；触摸板两指滚动同样产生按钮 4/5）
            while let Ok(Some(event)) = conn.poll_for_event() {
                let detail = match event {
                    X11Event::ButtonPress(ev) => Some(u32::from(ev.detail)),
                    X11Event::ButtonRelease(_) => None,
                    X11Event::MotionNotify(_) => None,
                    X11Event::EnterNotify(_) => None,
                    X11Event::LeaveNotify(_) => None,
                    other => {
                        eprintln!("[卫星] OTHER EVENT: {:?}", other);
                        None
                    }
                };
                match detail {
                    Some(4) => wheel.1 += 1.0,
                    Some(5) => wheel.1 -= 1.0,
                    Some(6) => wheel.0 -= 1.0,
                    Some(7) => wheel.0 += 1.0,
                    _ => {}
                }
            }
            if wheel.0 != 0.0 || wheel.1 != 0.0 {
                if let Ok(mut out) = stdout.lock() {
                    let _ = writeln!(out, "w {} {}", wheel.0, wheel.1);
                    let _ = out.flush();
                }
                wheel = (0.0, 0.0);
            }
        }

        let Ok(cookie) = conn.query_pointer(root) else {
            std::thread::sleep(std::time::Duration::from_millis(16));
            continue;
        };
        let Ok(reply) = cookie.reply() else {
            std::thread::sleep(std::time::Duration::from_millis(16));
            continue;
        };
        let pos = (reply.root_x, reply.root_y);

        if pos != last {
            last = pos;
            if let Ok(mut out) = stdout.lock() {
                let _ = writeln!(out, "{} {}", pos.0, pos.1);
                let _ = out.flush();
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(8));
    }
}

/// 根窗选中核心协议按钮事件（滚轮 = 按钮 4/5/6/7）。返回是否成功（失败降级为无滚轮）。
///
/// 注意：不用 XI2 raw 事件——xwayland-satellite（合成器桥）不产生 raw 事件（raw 源自
/// 设备驱动层真实硬件输入），合成器只把触摸板两指滚动转成按钮 4/5 的普通 press/release 对。
/// 核心协议 BUTTON_PRESS 是所有 X server 必须实现的。
fn setup_wheel_capture(conn: &impl Connection, root: x11rb::protocol::xproto::Window) -> bool {
    // 核心协议按钮事件（xdotool XTEST 验证通过）
    let core_aux = x11rb::protocol::xproto::ChangeWindowAttributesAux::new().event_mask(
        x11rb::protocol::xproto::EventMask::BUTTON_PRESS
            | x11rb::protocol::xproto::EventMask::BUTTON_RELEASE,
    );
    if let Err(e) = conn.change_window_attributes(root, &core_aux) {
        eprintln!("[卫星] ChangeWindowAttributes 失败({e})，滚轮转发禁用");
        return false;
    }
    true
}

// ── Phase 1: Boot ────────────────────────────────────────────────────

#[derive(Component)]
struct BootScreen;

#[derive(Component)]
struct BootBarFill;

fn spawn_boot_screen(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.spawn((
        BootScreen,
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            row_gap: Val::Px(40.0),
            ..default()
        },
        BackgroundColor(BG_DARK),
    ))
    .with_children(|parent| {
        // Logo
        parent.spawn((
            ImageNode::new(asset_server.load("nori/icon_00.png")),
            Node {
                width: Val::Px(120.0),
                height: Val::Px(120.0),
                ..default()
            },
        ));

        // Linear progress bar (track + fill)
        parent
            .spawn(Node {
                width: Val::Px(200.0),
                height: Val::Px(3.0),
                border_radius: BorderRadius::all(Val::Px(1.5)),
                overflow: Overflow::clip(),
                ..default()
            })
            .with_children(|bar| {
                bar.spawn((
                    BootBarFill,
                    Node {
                        width: Val::Px(0.0),
                        height: Val::Px(3.0),
                        ..default()
                    },
                    BackgroundColor(CYAN),
                ));
            });
    });
}

fn update_boot_screen(
    time: Res<Time>,
    mut boot_state: ResMut<BootState>,
    mut next_state: ResMut<NextState<OsState>>,
    mut fill_q: Query<&mut Node, With<BootBarFill>>,
) {
    boot_state.elapsed += time.delta_secs();
    boot_state.progress = (boot_state.elapsed / 2.0).min(1.0);

    for mut node in fill_q.iter_mut() {
        node.width = Val::Px(boot_state.progress * 200.0);
    }

    if boot_state.progress >= 1.0 {
        next_state.set(OsState::Loading);
    }
}

// ── Phase 2: Loading ─────────────────────────────────────────────────

#[derive(Component)]
struct LoadingScreen;

#[derive(Component)]
struct LoadingRing;

fn spawn_loading_screen(
    mut commands: Commands,
    query: Query<Entity, With<BootScreen>>,
    asset_server: Res<AssetServer>,
) {
    for entity in query.iter() {
        commands.entity(entity).despawn();
    }

    commands.spawn((
        LoadingScreen,
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            row_gap: Val::Px(40.0),
            ..default()
        },
        BackgroundColor(BG_DARK),
    ))
    .with_children(|parent| {
        // Flower icon
        parent.spawn((
            ImageNode::new(asset_server.load("nori/icon.png")),
            Node {
                width: Val::Px(120.0),
                height: Val::Px(120.0),
                ..default()
            },
        ));

        // Circular progress ring — single circle with per-side border colors
        parent.spawn((
            LoadingRing,
            Node {
                width: Val::Px(48.0),
                height: Val::Px(48.0),
                border_radius: BorderRadius::all(Val::Px(24.0)),
                border: UiRect::all(Val::Px(3.0)),
                ..default()
            },
            BorderColor::all(TRACK_COLOR),
        ));
    });
}

fn update_loading_screen(
    time: Res<Time>,
    mut load_state: ResMut<LoadState>,
    mut next_state: ResMut<NextState<OsState>>,
    mut ring_q: Query<&mut BorderColor, With<LoadingRing>>,
) {
    load_state.elapsed += time.delta_secs();
    load_state.progress = (load_state.elapsed / 2.0).min(1.0);

    let arc_center = (load_state.elapsed * 360.0) % 360.0;

    for mut border in ring_q.iter_mut() {
        border.top = arc_side_color(arc_center, 0.0);
        border.right = arc_side_color(arc_center, 90.0);
        border.bottom = arc_side_color(arc_center, 180.0);
        border.left = arc_side_color(arc_center, 270.0);
    }

    if load_state.progress >= 1.0 {
        next_state.set(OsState::Desktop);
    }
}

fn arc_side_color(arc_center: f32, side_center: f32) -> Color {
    let diff = angle_diff(arc_center, side_center).abs();
    // Arc is 90° wide, each side spans 90°. Coverage = overlap / 90°.
    let coverage = (1.0 - diff / 90.0).clamp(0.0, 1.0);
    lerp_color(TRACK_COLOR, CYAN, coverage)
}

fn angle_diff(from: f32, to: f32) -> f32 {
    let mut d = (to - from) % 360.0;
    if d > 180.0 { d -= 360.0; }
    if d < -180.0 { d += 360.0; }
    d
}

fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    let a = a.to_srgba();
    let b = b.to_srgba();
    Color::srgba(
        a.red + (b.red - a.red) * t,
        a.green + (b.green - a.green) * t,
        a.blue + (b.blue - a.blue) * t,
        1.0,
    )
}

// ── Desktop ──────────────────────────────────────────────────────────

#[derive(Component)]
struct DesktopScreen;

fn spawn_desktop_screen(
    mut commands: Commands,
    query: Query<Entity, With<LoadingScreen>>,
    asset_server: Res<AssetServer>,
    fonts: Res<N3riFonts>,
    pet_image: Res<PetDisplayImage>,
    pet_head: Res<PetHeadImage>,
    view_size: Res<n3ri_live2d::PetViewSize>,
    windows: Query<&Window, With<PrimaryWindow>>,
    wallpaper_surface: Option<Res<bevy_live_wallpaper::WallpaperSurfaceInfo>>,
    mut images: ResMut<Assets<Image>>,
    mut bg_materials: ResMut<Assets<DesktopBackgroundMaterial>>,
) {
    for entity in query.iter() {
        commands.entity(entity).despawn();
    }

    // 宠物节点一次定格：窗口模式 = 视口物理尺寸/scale × 0.75；壁纸模式 = surface 逻辑 × 0.75
    let pet_node_size = match windows.single() {
        Ok(window) => {
            let scale = window.scale_factor().max(1.0);
            Vec2::new(
                view_size.w as f32 / scale * n3ri_live2d::renderer::PET_DISPLAY_RATIO,
                view_size.h as f32 / scale * n3ri_live2d::renderer::PET_DISPLAY_RATIO,
            )
        }
        Err(_) => match wallpaper_surface.as_ref() {
            Some(surface) if surface.size.x > 1.0 => surface.size * n3ri_live2d::renderer::PET_DISPLAY_RATIO,
            _ => Vec2::new(600.0, 450.0),
        },
    };

    commands
        .spawn((
            DesktopScreen,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                display: Display::Flex,
                ..default()
            },
            BackgroundColor(BG_DARK),
        ))
        .with_children(|parent| {
            n3ri_ui::desktop::spawn_desktop_background(
                parent,
                &asset_server,
                &mut images,
                &mut bg_materials,
            );
            if let Some(image) = pet_image.0.clone() {
                spawn_pet_display(parent, &image, pet_node_size);
            }
            n3ri_ui::topbar::spawn_topbar(parent, &asset_server, &fonts);
            n3ri_ui::dock::spawn_dock(parent, &asset_server, &fonts);
            n3ri_ui::chat_capsule::spawn_chat_capsule(parent, &fonts);
            if let Some(head_image) = pet_head.0.clone() {
                spawn_head_display(parent, &head_image);
            }
        });
}

const OCCLUSION_THRESHOLD: f32 = 0.5;

fn toggle_head_display(
    pet: Query<(&ComputedNode, &UiGlobalTransform), With<PetDisplayNode>>,
    app_windows: Query<(&ComputedNode, &UiGlobalTransform, &Visibility), With<AppWindow>>,
    mut wanted: ResMut<HeadDisplayWanted>,
) {
    let occluded = match pet.single() {
        Ok((pet_node, pet_tf)) => {
            let pet_size = pet_node.size();
            let pet_area = pet_size.x * pet_size.y;
            if pet_area <= 0.0 {
                false
            } else {
                let pet_center = pet_tf.to_scale_angle_translation().2;
                let pet_min = pet_center - pet_size * 0.5;
                let pet_max = pet_center + pet_size * 0.5;

                let mut covered = 0.0_f32;
                for (win_node, win_tf, vis) in app_windows.iter() {
                    if *vis == Visibility::Hidden {
                        continue;
                    }
                    let half = win_node.size() * 0.5;
                    let win_center = win_tf.to_scale_angle_translation().2;
                    let overlap = (pet_max.min(win_center + half) - pet_min.max(win_center - half))
                        .max(Vec2::ZERO);
                    covered += overlap.x * overlap.y;
                }
                covered / pet_area >= OCCLUSION_THRESHOLD
            }
        }
        Err(_) => false,
    };
    if wanted.0 != occluded {
        wanted.0 = occluded;
    }
}

fn chat_rise_sync(
    head_query: Query<&Visibility, With<HeadDisplay>>,
    mut rise: ResMut<ChatRise>,
) {
    let head_visible = head_query
        .iter()
        .any(|v| *v == Visibility::Inherited || *v == Visibility::Visible);
    let target = if head_visible { 158.0 } else { 0.0 };
    if (rise.0 - target).abs() > 0.5 {
        rise.0 = target;
    }
}

const EMOTION_EXPRESSIONS: [(&str, &str); 9] = [
    ("happy", "07_Smile"),
    ("sad", "05_Dark"),
    ("angry", "03_Angry"),
    ("surprised", "02_Dizzy"),
    ("confused", "06_Speechless"),
    ("proud", "01_KiraKira"),
    ("shy", "04_Shy"),
    ("tired", "09_Troubled"),
    ("neutral", "00_Default"),
];

fn chat_emotion_bridge(
    mut emotion_events: MessageReader<ChatEmotionEvent>,
    mut pet: NonSendMut<Live2dPet>,
    time: Res<Time>,
) {
    for event in emotion_events.read() {
        if let Some((_, name)) = EMOTION_EXPRESSIONS
            .iter()
            .find(|(key, _)| key == &event.0.as_str())
        {
            pet.start_expression(name, time.elapsed_secs());
        }
    }
}
