//! System tray icon for background operation.
//!
//! Uses [`ksni`](https://crates.io/crates/ksni) on Linux (pure Rust DBus
//! StatusNotifierItem, no GTK), and [`tray-icon`](https://crates.io/crates/tray-icon)
//! on macOS/Windows (native platform APIs).

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc;
use std::sync::Arc;

/// Pet mode state shared between main thread and tray.
/// 0 = Off, 1 = Windowed, 2 = AlwaysOnTop
pub type PetModeState = Arc<AtomicU8>;
pub const PET_OFF: u8 = 0;
pub const PET_WINDOWED: u8 = 1;
pub const PET_ALWAYS_ON_TOP: u8 = 2;

/// Load the embedded app icon (from `res/icon.png`) as raw RGBA8 pixels.
///
/// Shared by both the Linux (`ksni`) and non-Linux (`tray-icon`) backends.
/// The PNG is compiled into the binary via [`include_bytes!`].
fn embedded_icon_rgba() -> (Vec<u8>, u32, u32) {
    let png = include_bytes!("../res/icon.png");
    let img = image::load_from_memory(png)
        .expect("embedded tray icon (res/icon.png)")
        .into_rgba8();
    let (w, h) = img.dimensions();
    (img.into_raw(), w, h)
}

/// Custom events sent from the tray to the winit event loop.
#[derive(Debug, Clone)]
pub enum AppEvent {
    ShowWindow,
    Quit,
    ToggleClickThrough,
    ToggleWindowedPet,
    ToggleAlwaysOnTopPet,
}

/// Wraps an [`mpsc::Receiver`] and exposes a non-blocking [`poll`](Self::poll).
pub struct MenuEventReceiver {
    rx: mpsc::Receiver<String>,
}

impl MenuEventReceiver {
    /// Block until a menu event arrives. Used by the background forwarding thread.
    pub fn recv(&self) -> Result<String, mpsc::RecvError> {
        self.rx.recv()
    }
}

// ── Linux: ksni (pure Rust DBus StatusNotifierItem) ────────────────

#[cfg(target_os = "linux")]
mod tray_imp {
    use super::*;
    use ksni::blocking::TrayMethods;

    pub struct Live2dTray {
        pub tx: mpsc::Sender<String>,
        pub pet_state: PetModeState,
    }

    impl ksni::Tray for Live2dTray {
        fn id(&self) -> String {
            "live2d-viewer".into()
        }

        fn icon_pixmap(&self) -> Vec<ksni::Icon> {
            let (rgba, w, h) = embedded_icon_rgba();
            // ksni expects ARGB byte order
            let data: Vec<u8> = rgba
                .chunks_exact(4)
                .flat_map(|p| [p[3], p[0], p[1], p[2]])
                .collect();
            vec![ksni::Icon {
                width: w as i32,
                height: h as i32,
                data,
            }]
        }

        fn tool_tip(&self) -> ksni::ToolTip {
            ksni::ToolTip {
                title: "Live2D Pet".into(),
                ..Default::default()
            }
        }

        fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
            use ksni::menu::*;
            vec![
                StandardItem {
                    label: "Show Window".into(),
                    activate: Box::new(|this: &mut Live2dTray| {
                        let _ = this.tx.send("show".into());
                    }),
                    ..Default::default()
                }
                .into(),
                StandardItem {
                    label: "Toggle Click-Through".into(),
                    activate: Box::new(|this: &mut Live2dTray| {
                        let _ = this.tx.send("clickthrough".into());
                    }),
                    ..Default::default()
                }
                .into(),
                // ── Pet mode toggles (mutually exclusive) ──
                StandardItem {
                    label: match self.pet_state.load(Ordering::Relaxed) {
                        PET_WINDOWED => "✓ Windowed Pet".into(),
                        _ => "  Windowed Pet".into(),
                    },
                    activate: Box::new(|this: &mut Live2dTray| {
                        let _ = this.tx.send("windowed_pet".into());
                    }),
                    ..Default::default()
                }
                .into(),
                StandardItem {
                    label: match self.pet_state.load(Ordering::Relaxed) {
                        PET_ALWAYS_ON_TOP => "✓ Always on Top Pet".into(),
                        _ => "  Always on Top Pet".into(),
                    },
                    activate: Box::new(|this: &mut Live2dTray| {
                        let _ = this.tx.send("alwaysontop_pet".into());
                    }),
                    ..Default::default()
                }
                .into(),
                StandardItem {
                    label: "Quit".into(),
                    activate: Box::new(|this: &mut Live2dTray| {
                        let _ = this.tx.send("quit".into());
                    }),
                    ..Default::default()
                }
                .into(),
            ]
        }
    }

    pub fn create_tray(
        pet_state: PetModeState,
    ) -> (
        Option<ksni::blocking::Handle<Live2dTray>>,
        MenuEventReceiver,
    ) {
        let (tx, rx) = mpsc::channel();
        let tray = Live2dTray { tx, pet_state };
        let handle = match tray.spawn() {
            Ok(h) => {
                log::info!("Tray icon registered");
                Some(h)
            }
            Err(e) => {
                log::warn!(
                    "Tray icon not available (StatusNotifierWatcher not found on D-Bus): {e}; \
                     tray menu disabled"
                );
                None
            }
        };
        (handle, MenuEventReceiver { rx })
    }
}

#[cfg(target_os = "linux")]
pub use tray_imp::create_tray;

// ── Non-Linux: tray-icon (macOS NSStatusBar / Windows Shell) ───────

#[cfg(not(target_os = "linux"))]
mod tray_imp {
    use super::*;
    use tray_icon;
    use tray_icon::menu::{Menu, MenuEvent, MenuItem};
    use tray_icon::{Icon, TrayIconBuilder};

    pub fn create_tray(_pet_state: PetModeState) -> (tray_icon::TrayIcon, MenuEventReceiver) {
        let menu = Menu::new();
        let show_item = MenuItem::with_id("show", "Show Window", true, None);
        let clickthrough_item =
            MenuItem::with_id("clickthrough", "Toggle Click-Through", true, None);
        let quit_item = MenuItem::with_id("quit", "Quit", true, None);
        menu.append_items(&[&show_item, &clickthrough_item, &quit_item])
            .ok();

        let (rgba, w, h) = embedded_icon_rgba();
        let icon = Icon::from_rgba(rgba, w, h).expect("create tray icon");
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Live2D Pet")
            .with_icon(icon)
            .build()
            .expect("build tray icon");

        let (tx, rx) = mpsc::channel();
        MenuEvent::set_event_handler(Some(Box::new(move |event: MenuEvent| {
            let id = event.id.0.clone();
            let _ = tx.send(id);
        })));

        (tray, MenuEventReceiver { rx })
    }
}

#[cfg(not(target_os = "linux"))]
pub use tray_imp::create_tray;
