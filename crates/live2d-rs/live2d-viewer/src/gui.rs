use crate::app::{AppState, PetMode};
use crate::theme;
use egui::{Context, Slider, Window};
use std::path::PathBuf;

pub fn draw_ui(ctx: &Context, app: &mut AppState) {
    // Apply theme if preset or dark/light changed
    if app.theme_needs_refresh {
        app.theme_needs_refresh = false;
        let builder = match app.theme_preset {
            0 => theme::Theme::aira(),
            1 => theme::Theme::coral(),
            2 => theme::Theme::mint(),
            _ => theme::Theme::purple(),
        };
        let t = if app.theme_dark {
            builder.dark()
        } else {
            builder.light()
        };
        theme::apply_theme(ctx, &t);
    }

    // Poll for AI response (non-blocking, receives from background thread)
    app.poll_ai_result();
    // Play completed TTS audio
    app.poll_tts_result();
    // Auto-reset AI emotion timeout (runs every frame regardless of AI stream)
    app.tick_emotion_timeout();
    // Handle deferred TTS voice refresh (outside any window closure to avoid borrow conflict)
    if app.tts_refresh_requested {
        app.tts_refresh_requested = false;
        app.refresh_tts_voices();
    }

    if app.minimized_to_float {
        draw_floating_ui(ctx, app);
        return;
    }

    if app.pet_mode != PetMode::Off {
        if app.pet_mode == PetMode::Windowed || app.pet_mode == PetMode::AlwaysOnTop {
            draw_pet_ui(ctx, app);
        }
    } else {
        draw_normal_ui(ctx, app);
    }

    // HitArea debug overlay (yellow boxes), toggle in Settings
    if app.debug_hit_areas && app.pet_mode == PetMode::Off {
        draw_hit_area_debug(ctx, app);
    }

    // Error window always shows regardless of mode
    if let Some(err) = app.error_message.clone() {
        Window::new("错误").show(ctx, |ui| {
            ui.colored_label(egui::Color32::RED, &err);
            let close_btn = theme::button(ui, theme::ButtonVariant::Secondary, "关闭");
            if ui.add(close_btn).clicked() {
                app.error_message = None;
            }
        });
    }

    // Settings window — accessible from all modes
    if app.settings_open {
        draw_settings(ctx, app);
    }
    // AI Chat window (normal mode only)
    crate::ai::chat_panel::draw_chat_panel(ctx, app);
    // Character card editor window
    crate::ai::character_card_panel::draw_character_card_panel(ctx, app);

    // Screen capture preview window
    #[cfg(feature = "capture")]
    draw_capture_preview(ctx, app);

    // Toggle buttons in top-right corner
    egui::Area::new("toggle_btns".into())
        .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-4.0, 4.0))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                if app.ai_enabled && app.pet_mode == PetMode::Off {
                    let chat_label = if app.ai_chat_open {
                        "\u{1F4AC}"
                    } else {
                        "\u{1F4AD}"
                    };
                    if ui.button(chat_label).clicked() {
                        app.ai_chat_open = !app.ai_chat_open;
                    }
                }
                #[cfg(feature = "capture")]
                {
                    let cap_label = if app.is_capturing() {
                        "\u{1F534}"
                    } else {
                        "\u{26AA}"
                    };
                    if ui.button(cap_label).clicked() {
                        if app.is_capturing() {
                            app.stop_capture();
                        } else {
                            app.start_capture();
                        }
                    }
                    if app.is_capturing() && ui.button("\u{1F4F7}").clicked() {
                        app.trigger_vision_snapshot();
                    }
                }
                if ui.button("\u{2699}").clicked() {
                    app.settings_open = !app.settings_open;
                }
            });
        });
}

fn draw_floating_ui(ctx: &Context, app: &mut AppState) {
    let screen = ctx.screen_rect();

    let btn_size = screen.size().x.min(screen.size().y) - 4.0;
    let btn_rect = egui::Rect::from_center_size(screen.center(), egui::vec2(btn_size, btn_size));
    let icon_size = btn_size * 0.45;

    let painter = ctx.debug_painter();
    // Draw a RED rect to contrast with blue GL clear
    painter.rect_filled(btn_rect, 4.0, egui::Color32::RED);
    painter.text(
        btn_rect.center(),
        egui::Align2::CENTER_CENTER,
        "\u{25b6}",
        egui::FontId::proportional(icon_size),
        egui::Color32::WHITE,
    );

    if ctx.input(|i| i.pointer.any_click()) {
        let pos = match ctx.input(|i| i.pointer.interact_pos()) {
            Some(p) => p,
            None => return,
        };
        if btn_rect.contains(pos) {
            app.request_restore = true;
        }
    }
}

fn draw_normal_ui(ctx: &Context, app: &mut AppState) {
    let current_idx = app.current_idx;

    // Collect model info first to avoid borrow conflict with switch_to
    let model_names: Vec<String> = app.model_list.iter().map(|e| e.name.clone()).collect();

    Window::new("Model List")
        .default_width(280.0)
        .default_height(360.0)
        .scroll2([false, true])
        .show(ctx, |ui| {
            ui.set_max_width(260.0);
            let mut deleted_idx = None;
            let mut switched_idx = None;

            let row_h = 24.0;
            egui::ScrollArea::vertical().max_height(400.0).show_rows(
                ui,
                row_h,
                model_names.len(),
                |ui, range| {
                    for i in range {
                        let name = &model_names[i];
                        let selected = current_idx == Some(i);
                        let is_renaming = app.renaming_idx == Some(i);

                        ui.horizontal(|ui| {
                            ui.set_max_width(250.0);
                            ui.set_min_height(22.0);

                            if is_renaming {
                                let resp = ui.add_sized(
                                    egui::vec2(140.0, 20.0),
                                    egui::TextEdit::singleline(&mut app.renaming_buffer)
                                        .desired_width(140.0),
                                );
                                if resp.lost_focus()
                                    || ui.input(|i| i.key_pressed(egui::Key::Enter))
                                {
                                    let new_name = app.renaming_buffer.trim().to_string();
                                    if !new_name.is_empty() && new_name != *name {
                                        if let Some(ref db) = app.db {
                                            if let Some(entry) = app.model_list.get(i) {
                                                let path = entry.dir.to_string_lossy().to_string();
                                                let _ = db.rename_model(&path, &new_name);
                                            }
                                        }
                                        app.model_list[i].name = new_name;
                                    }
                                    app.renaming_idx = None;
                                }
                            } else {
                                let short_name = if name.chars().count() > 24 {
                                    let truncated: String = name.chars().take(23).collect();
                                    format!("{truncated}…")
                                } else {
                                    name.clone()
                                };
                                let label = format!(
                                    "{} {}",
                                    if selected { "\u{25cf}" } else { "\u{25cb}" },
                                    short_name,
                                );
                                let resp = ui.add_sized(
                                    egui::vec2(190.0, 20.0),
                                    egui::SelectableLabel::new(selected, label),
                                );
                                if resp.clicked() {
                                    switched_idx = Some(i);
                                }
                                if resp.double_clicked() {
                                    app.renaming_idx = Some(i);
                                    app.renaming_buffer = name.clone();
                                }
                            }

                            let btn =
                                egui::Button::new("\u{270f}").min_size(egui::vec2(18.0, 18.0));
                            if !is_renaming && ui.add(btn).clicked() {
                                app.renaming_idx = Some(i);
                                app.renaming_buffer = name.clone();
                            }

                            let del =
                                egui::Button::new("\u{2716}").min_size(egui::vec2(18.0, 18.0));
                            if ui.add(del).clicked() {
                                deleted_idx = Some(i);
                            }
                        });
                    }
                },
            );

            // Deferred: process outside the loop to avoid borrow conflict with model_list
            if let Some(i) = deleted_idx {
                if let Some(ref db) = app.db {
                    if let Some(entry) = app.model_list.get(i) {
                        let path = entry.dir.to_string_lossy().to_string();
                        let _ = db.remove_model(&path);
                    }
                }
                app.model_list.remove(i);
                if app.current_idx == Some(i) {
                    app.current_idx = if app.model_list.is_empty() {
                        None
                    } else {
                        Some(i.min(app.model_list.len() - 1))
                    };
                } else if let Some(ci) = app.current_idx {
                    if ci > i {
                        app.current_idx = Some(ci - 1);
                    }
                }
            }
            if let Some(i) = switched_idx {
                if let Err(e) = app.begin_switch(i) {
                    app.error_message = Some(e);
                }
            }

            ui.separator();
            let btn = theme::button(ui, theme::ButtonVariant::Primary, "Add Model...");
            if ui.add(btn).clicked() {
                if let Some(path) = rfd::FileDialog::new().pick_folder() {
                    app.add_model_dir(path);
                }
            }

            ui.separator();

            // Show loading indicator during async model switch
            if matches!(app.pending_load, crate::app::PendingLoad::V3Loading(..)) {
                ui.label(egui::RichText::new("加载中...").color(egui::Color32::YELLOW));
            }

            ui.separator();
            let pet_btn = theme::button(
                ui,
                theme::ButtonVariant::Secondary,
                "\u{1f43e} Windowed Pet",
            );
            if ui.add(pet_btn).clicked() {
                if app.pet_mode == PetMode::Windowed {
                    app.pet_mode = PetMode::Off;
                } else {
                    app.pet_mode = PetMode::Windowed;
                }
                app.pet_mode_changed = true;
            }
            let top_btn = theme::button(
                ui,
                theme::ButtonVariant::Secondary,
                "\u{1f43e} Always on Top",
            );
            if ui.add(top_btn).clicked() {
                if app.pet_mode == PetMode::AlwaysOnTop {
                    app.pet_mode = PetMode::Off;
                } else {
                    app.pet_mode = PetMode::AlwaysOnTop;
                }
                app.pet_mode_changed = true;
            }

            // Zoom controls — always visible regardless of model type
            ui.separator();
            ui.horizontal(|ui| {
                let zoom_out = theme::button(ui, theme::ButtonVariant::Secondary, "-");
                if ui.add(zoom_out).clicked() {
                    if app.is_v2 {
                        app.v2_scale = (app.v2_scale * 0.85).max(0.1);
                        if let Some(ref mut v2) = app.v2_model {
                            v2.set_scale(app.v2_scale);
                        }
                    } else {
                        app.camera.zoom_out();
                    }
                    app.save_zoom();
                }
                let reset_btn = theme::button(ui, theme::ButtonVariant::Secondary, "Reset");
                if ui.add(reset_btn).clicked() {
                    if app.is_v2 {
                        app.v2_scale = 1.0;
                        if let Some(ref mut v2) = app.v2_model {
                            v2.set_scale(1.0);
                            v2.set_offset(0.0, 0.0);
                        }
                    } else {
                        app.camera.reset_pan();
                    }
                    app.save_zoom();
                }
                let zoom_in = theme::button(ui, theme::ButtonVariant::Secondary, "+");
                if ui.add(zoom_in).clicked() {
                    if app.is_v2 {
                        app.v2_scale = (app.v2_scale * 1.15).min(10.0);
                        if let Some(ref mut v2) = app.v2_model {
                            v2.set_scale(app.v2_scale);
                        }
                    } else {
                        app.camera.zoom_in();
                    }
                    app.save_zoom();
                }
            });
        });

    if app.current_model.is_some() {
        Window::new("Parameters")
            .default_width(300.0)
            .default_height(400.0)
            .scroll2([false, true])
            .show(ctx, |ui| {
                ui.label(format!("Parameters: {}", app.parameter_names.len()));

                theme::collapsible(ui, "状态信息", &mut app.panel_motion_open, |ui| {
                    let total_entries = app.motion_queue.entries.len();
                    if total_entries > 0 {
                        ui.label(format!("Motions: {}", total_entries));
                        for (i, entry) in app.motion_queue.entries.iter().enumerate() {
                            let loop_str = if entry.motion.is_loop {
                                "循环"
                            } else {
                                "一次"
                            };
                            let fw = entry.cached_fade_weight;
                            ui.label(format!(
                                "  [{}:{}] ({:.1}s, {}, fade={:.2})",
                                i, i, entry.motion.data.duration, loop_str, fw,
                            ));
                        }
                    }

                    // Expression status
                    if app.expression_manager.is_active {
                        ui.label("表情: 启用");
                    }
                });

                // Action buttons
                ui.horizontal(|ui| {
                    let idle_btn = theme::button(ui, theme::ButtonVariant::Primary, "重放待机");
                    if ui.add(idle_btn).clicked() {
                        app.auto_play_idle = true;
                        app.start_motion("Idle", Some(0));
                    }
                    let stop_btn = theme::button(ui, theme::ButtonVariant::Danger, "全部停止");
                    if ui.add(stop_btn).clicked() {
                        app.motion_queue.stop_all_motions();
                    }
                });

                // V2 models have motions loaded internally by the C++ wrapper.
                // V3 models: try "TapBody" first, then fall back to "".
                let has_tap_motion = app.is_v2
                    || app
                        .loaded_motions
                        .get("TapBody")
                        .or_else(|| app.loaded_motions.get(""))
                        .is_some_and(|m| !m.is_empty());
                let body_btn = theme::button(ui, theme::ButtonVariant::Secondary, "点击身体");
                if has_tap_motion && ui.add(body_btn).clicked() {
                    if app.is_v2 {
                        // V2: motions are internal to C++ wrapper; cycle through indices
                        let idx = (app.motion_queue.user_time_seconds as usize) % 3;
                        app.start_motion("TapBody", Some(idx));
                    } else {
                        let user_time = app.motion_queue.user_time_seconds;
                        let motions = app
                            .loaded_motions
                            .get("TapBody")
                            .or_else(|| app.loaded_motions.get(""))
                            .unwrap();
                        let idx = (user_time as usize) % motions.len();
                        app.start_motion("TapBody", Some(idx));
                    }
                }

                // UserData info: show description of last tapped area
                if let Some(ref info) = app.last_tapped_user_data {
                    ui.label(
                        egui::RichText::new(info)
                            .color(egui::Color32::LIGHT_BLUE)
                            .size(13.0),
                    );
                    let dismiss_btn = theme::button(ui, theme::ButtonVariant::Danger, "✕");
                    if ui.add(dismiss_btn).clicked() {
                        app.last_tapped_user_data = None;
                    }
                    ui.separator();
                }

                // ── Parameter presets ──
                ui.horizontal(|ui| {
                    ui.add_sized(
                        [120.0, 0.0],
                        egui::TextEdit::singleline(&mut app.preset_name_input).hint_text("预设名"),
                    );
                    let save_btn = theme::button(ui, theme::ButtonVariant::Primary, "保存预设");
                    if ui.add(save_btn).clicked() {
                        let name = app.preset_name_input.trim().to_string();
                        if !name.is_empty() {
                            app.save_preset(&name);
                            app.preset_name_input.clear();
                        }
                    }
                });
                if !app.preset_list.is_empty() {
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .max_height(120.0)
                        .show(ui, |ui| {
                            let mut to_delete: Option<String> = None;
                            let mut to_load: Option<String> = None;
                            for name in &app.preset_list {
                                ui.horizontal(|ui| {
                                    ui.label(name);
                                    let load_btn =
                                        theme::button(ui, theme::ButtonVariant::Secondary, "→");
                                    if ui.add(load_btn).clicked() {
                                        to_load = Some(name.clone());
                                    }
                                    let del_btn =
                                        theme::button(ui, theme::ButtonVariant::Danger, "✕");
                                    if ui.add(del_btn).clicked() {
                                        to_delete = Some(name.clone());
                                    }
                                });
                            }
                            if let Some(n) = to_load {
                                app.load_preset(&n);
                            }
                            if let Some(n) = to_delete {
                                app.delete_preset(&n);
                            }
                        });
                }

                ui.separator();

                // ── Layout mode toggle ──
                let layout_btn = theme::button(ui, theme::ButtonVariant::Secondary, "调整布局");
                if ui.add(layout_btn).clicked() {
                    app.layout_mode = !app.layout_mode;
                }

                if app.layout_mode {
                    ui.separator();
                    ui.label("布局模式");
                    let mut px = app.camera.translate_x;
                    let mut py = app.camera.translate_y;
                    let mut zm = (app.camera.scale_x.abs() + app.camera.scale_y.abs()) / 2.0;
                    let p_changed = ui
                        .add(Slider::new(&mut px, -5.0..=5.0).text("X 偏移"))
                        .changed()
                        | ui.add(Slider::new(&mut py, -5.0..=5.0).text("Y 偏移"))
                            .changed();
                    let z_changed = ui
                        .add(Slider::new(&mut zm, 0.1..=5.0).text("缩放"))
                        .changed();
                    if p_changed {
                        app.camera.translate_x = px;
                        app.camera.translate_y = py;
                    }
                    if z_changed {
                        let sign_x = app.camera.scale_x.signum();
                        let sign_y = app.camera.scale_y.signum();
                        app.camera.scale_x = sign_x * zm;
                        app.camera.scale_y = sign_y * zm;
                    }
                    ui.horizontal(|ui| {
                        let save_l = theme::button(ui, theme::ButtonVariant::Primary, "保存布局");
                        if ui.add(save_l).clicked() {
                            app.save_layout();
                            app.layout_mode = false;
                        }
                        let reset_l = theme::button(ui, theme::ButtonVariant::Caution, "重置布局");
                        if ui.add(reset_l).clicked() {
                            app.camera.translate_x = 0.0;
                            app.camera.translate_y = 0.0;
                            app.layout_mode = false;
                        }
                    });
                    ui.separator();
                }

                // Parameter sliders in scroll area
                egui::ScrollArea::vertical()
                    .max_height(400.0)
                    .show(ui, |ui| {
                        for i in 0..app.parameter_names.len() {
                            let name = &app.parameter_names[i];
                            let min = app.parameter_mins.get(i).copied().unwrap_or(-1.0);
                            let max = app.parameter_maxs.get(i).copied().unwrap_or(1.0);
                            let mut val = app.parameter_values[i];
                            if ui
                                .add(Slider::new(&mut val, min..=max).text(name))
                                .changed()
                            {
                                app.parameter_values[i] = val;
                                app.update_parameters();
                            }
                        }
                    });
            }); // close Window::show
    } // close if app.current_model.is_some()

    // ── Motion & expression browser ──
    if app.current_idx.is_some() {
        Window::new("动作与表情")
            .default_width(240.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(400.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        let mut play_motion: Option<(String, usize)> = None;
                        let mut set_expression: Option<String> = None;
                        let mut clear_expression = false;
                        let mut stop_motions = false;

                        ui.collapsing("动作", |ui| {
                            if app.loaded_motions.is_empty() {
                                ui.label("(无已加载动作)");
                            }
                            let mut groups: Vec<&String> = app.loaded_motions.keys().collect();
                            groups.sort();
                            for group in groups {
                                let count = app.loaded_motions[group].len();
                                ui.label(format!("▶ {group}"));
                                ui.horizontal_wrapped(|ui| {
                                    for i in 0..count {
                                        let label = app
                                            .loaded_motion_names
                                            .get(group)
                                            .and_then(|v| v.get(i))
                                            .cloned()
                                            .unwrap_or_else(|| format!("motion {i}"));
                                        let active = matches!(&app.active_motion,
                                    Some((g, idx)) if g == group && *idx == i);
                                        let variant = if active {
                                            theme::ButtonVariant::Primary
                                        } else {
                                            theme::ButtonVariant::Secondary
                                        };
                                        if ui.add(theme::button(ui, variant, label)).clicked() {
                                            play_motion = Some((group.clone(), i));
                                        }
                                    }
                                });
                            }
                        });

                        ui.separator();

                        ui.collapsing("表情", |ui| {
                            if app.loaded_expressions.is_empty() {
                                ui.label("(无已加载表情)");
                            }
                            let mut names: Vec<&String> = app.loaded_expressions.keys().collect();
                            names.sort();
                            ui.horizontal_wrapped(|ui| {
                                for name in names {
                                    let active = app.active_expression_name.as_deref()
                                        == Some(name.as_str());
                                    let variant = if active {
                                        theme::ButtonVariant::Primary
                                    } else {
                                        theme::ButtonVariant::Secondary
                                    };
                                    if ui.add(theme::button(ui, variant, name.as_str())).clicked() {
                                        set_expression = Some(name.clone());
                                    }
                                }
                            });
                            if ui
                                .add(theme::button(ui, theme::ButtonVariant::Caution, "关闭表情"))
                                .clicked()
                            {
                                clear_expression = true;
                            }
                            if ui
                                .add(theme::button(ui, theme::ButtonVariant::Caution, "关闭动作"))
                                .clicked()
                            {
                                stop_motions = true;
                            }
                        });

                        // Apply actions after drawing (avoids borrow conflicts)
                        if let Some((group, idx)) = play_motion {
                            app.start_motion(&group, Some(idx));
                        }
                        if let Some(name) = set_expression {
                            if let Some(expr) = app.loaded_expressions.get(&name).cloned() {
                                app.expression_manager
                                    .start_expression(expr, app.motion_queue.user_time_seconds);
                            }
                        }
                        if clear_expression {
                            app.expression_manager.clear();
                            app.active_expression_name = None;
                        }
                        if stop_motions {
                            // Cancel manually triggered motions only; auto-idle keeps
                            // running as the default behaviour (physics stays alive).
                            app.motion_queue.stop_all_motions();
                            app.active_motion = None;
                        }
                    });
            });
    }

    // ── Search window ──
    let has_query = !app.search_query.is_empty();
    Window::new("搜索").default_width(260.0).show(ctx, |ui| {
        let prev = app.search_query.clone();
        ui.add(
            egui::TextEdit::singleline(&mut app.search_query)
                .hint_text("输入关键词搜索模型...")
                .desired_width(240.0),
        );

        // Trigger search when query changes
        let query_changed = app.search_query != prev;
        if query_changed {
            if app.search_query.is_empty() {
                app.search_results.clear();
            } else {
                // Results arrive via poll_search(); DB work stays off this thread.
                app.request_model_search();
            }
        }

        ui.separator();

        if app.search_results.is_empty() && has_query {
            let status = if app.search_inflight {
                "搜索中…"
            } else {
                "无匹配结果"
            };
            ui.label(status);
        }

        let mut switch_idx: Option<usize> = None;
        let results_clone = app.search_results.clone();
        egui::ScrollArea::vertical().max_height(350.0).show_rows(
            ui,
            24.0,
            results_clone.len(),
            |ui, range| {
                for i in range {
                    let result = &results_clone[i];
                    let pct = (result.similarity * 100.0).clamp(0.0, 100.0);
                    let label = format!("{}  ({:.0}%)", result.name, pct,);
                    if ui.selectable_label(false, &label).clicked() {
                        // Find the model in model_list by file_path
                        if let Some(idx) = app
                            .model_list
                            .iter()
                            .position(|e| e.dir.to_string_lossy() == result.file_path)
                        {
                            switch_idx = Some(idx);
                        }
                    }
                }
            },
        );

        if let Some(i) = switch_idx {
            if let Err(e) = app.begin_switch(i) {
                app.error_message = Some(e);
            }
        }
    });
}

fn draw_pet_ui(ctx: &Context, app: &mut AppState) {
    let current_idx = app.current_idx;

    let screen_rect = ctx.screen_rect();

    // Delay toolbar appearance to let window resize settle
    if app.pet_mode_delay > 0 {
        app.pet_mode_delay -= 1;
        return;
    }

    // Position toolbar at model's right edge, vertically centered.
    // Clamp to window bounds so wide-aspect models don't push the panel off-screen.
    let (toolbar_x, toolbar_y): (f32, f32) = {
        let (cw, ch) = app.canvas_pixel_size;
        let (ww, wh) = app.window_size;
        let logical_w = screen_rect.width();
        let logical_h = screen_rect.height();
        if cw > 0.0 && ch > 0.0 && ww > 0.0 {
            let sf = ww / logical_w;
            let model_display_w = cw / ch * wh;
            let model_right_px = (ww + model_display_w) / 2.0;
            let toolbar_w = 44.0;
            let x = (model_right_px / sf).clamp(2.0, logical_w - toolbar_w);
            let y = logical_h / 2.0 - 70.0; // vertically centered (toolbar ~140px tall)
            (x, y)
        } else {
            (logical_w - 36.0, 8.0)
        }
    };

    egui::Area::new("pet_toolbar".into())
        .fixed_pos(egui::pos2(toolbar_x, toolbar_y))
        .order(egui::Order::Foreground)
        .movable(false)
        .show(ctx, |ui| {
            let mut frame = egui::Frame::none();
            frame.fill = egui::Color32::from_black_alpha(20);
            frame.rounding = egui::Rounding::same(4.0);
            frame.inner_margin = egui::Margin::symmetric(2.0, 3.0);
            frame.show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    small_btn(ui, "\u{25c0}").clicked().then(|| {
                        if let Some(idx) = current_idx {
                            if idx > 0 {
                                let _ = app.begin_switch(idx - 1);
                            }
                        }
                    });

                    ui.label(
                        egui::RichText::new(
                            if matches!(app.pending_load, crate::app::PendingLoad::V3Loading(..)) {
                                "加载中..."
                            } else {
                                current_idx
                                    .and_then(|i| app.model_list.get(i))
                                    .map(|e| e.name.as_str())
                                    .unwrap_or("--")
                            },
                        )
                        .size(10.0)
                        .weak(),
                    );

                    small_btn(ui, "\u{25b6}").clicked().then(|| {
                        if let Some(idx) = current_idx {
                            if idx + 1 < app.model_list.len() {
                                let _ = app.begin_switch(idx + 1);
                            }
                        }
                    });

                    ui.add_space(3.0);

                    small_btn(ui, "\u{21ba}").clicked().then(|| {
                        if app.is_v2 {
                            app.v2_scale = 1.0;
                            if let Some(v2) = app.v2_model.as_mut() {
                                v2.set_scale(1.0);
                                v2.set_offset(0.0, 0.0);
                            }
                        } else {
                            app.camera.reset_pan();
                        }
                        app.save_zoom();
                    });
                    small_btn(ui, "+").clicked().then(|| {
                        if app.is_v2 {
                            app.v2_scale = (app.v2_scale * 1.1).min(10.0);
                            if let Some(v2) = app.v2_model.as_mut() {
                                v2.set_scale(app.v2_scale);
                            }
                        } else {
                            app.camera.zoom_in();
                        }
                        app.save_zoom();
                    });
                    small_btn(ui, "-").clicked().then(|| {
                        if app.is_v2 {
                            app.v2_scale = (app.v2_scale * 0.9).max(0.1);
                            if let Some(v2) = app.v2_model.as_mut() {
                                v2.set_scale(app.v2_scale);
                            }
                        } else {
                            app.camera.zoom_out();
                        }
                        app.save_zoom();
                    });

                    ui.add_space(3.0);

                    if small_btn(ui, "\u{1f50d}").clicked() {
                        app.pet_search_open = !app.pet_search_open;
                        if app.pet_search_open {
                            app.search_query.clear();
                            app.search_results.clear();
                        }
                    }

                    ui.add_space(3.0);

                    if ui
                        .add(egui::Button::new("\u{2193}").min_size(egui::vec2(30.0, 22.0)))
                        .on_hover_text("Minimize to tray")
                        .clicked()
                    {
                        app.request_minimize = true;
                    }

                    ui.add_space(3.0);

                    small_btn(ui, "\u{2716}").clicked().then(|| {
                        app.pet_mode = PetMode::Off;
                        app.pet_mode_changed = true;
                    });
                });
            });
        });

    // ── Pet toolbar search popup ──
    if app.pet_search_open {
        egui::Area::new("pet_search".into())
            .fixed_pos(egui::pos2((toolbar_x - 260.0).max(2.0), toolbar_y))
            .order(egui::Order::Foreground)
            .movable(false)
            .show(ctx, |ui| {
                let mut frame = egui::Frame::none();
                frame.fill = egui::Color32::from_black_alpha(180);
                frame.rounding = egui::Rounding::same(6.0);
                frame.inner_margin = egui::Margin::symmetric(6.0, 4.0);
                frame.show(ui, |ui| {
                    ui.set_min_width(240.0);
                    let prev = app.search_query.clone();
                    ui.add(
                        egui::TextEdit::singleline(&mut app.search_query)
                            .hint_text("搜索模型...")
                            .desired_width(228.0),
                    );

                    // Trigger search when query changes
                    let query_changed = app.search_query != prev;
                    if query_changed {
                        if app.search_query.is_empty() {
                            app.search_results.clear();
                        } else {
                            // Results arrive via poll_search(); DB work stays off this thread.
                            app.request_model_search();
                        }
                    }

                    ui.separator();

                    if !app.search_results.is_empty() {
                        let results = app.search_results.clone();
                        egui::ScrollArea::vertical().max_height(300.0).show_rows(
                            ui,
                            24.0,
                            results.len(),
                            |ui, range| {
                                for i in range {
                                    let result = &results[i];
                                    let pct = (result.similarity * 100.0).clamp(0.0, 100.0);
                                    let label = format!("{}  ({:.0}%)", result.name, pct);
                                    if ui
                                        .add(
                                            egui::Button::new(&label)
                                                .min_size(egui::vec2(228.0, 22.0)),
                                        )
                                        .clicked()
                                    {
                                        if let Some(idx) = app.model_list.iter().position(|e| {
                                            e.dir.to_string_lossy() == result.file_path
                                        }) {
                                            app.pet_search_open = false;
                                            if let Err(e) = app.begin_switch(idx) {
                                                app.error_message = Some(e);
                                            }
                                        }
                                    }
                                }
                            },
                        );
                    } else if !app.search_query.is_empty() {
                        ui.label("无匹配结果");
                    }
                });
            });
    }
}

/// Debug overlay: draw a yellow AABB around each model3.json HitArea
/// drawable, mirroring the official Cubism web viewer's debug rendering.
fn draw_hit_area_debug(ctx: &Context, app: &AppState) {
    let Some(model) = app.current_model.as_ref() else {
        return;
    };
    if app.hit_areas.is_empty() {
        return;
    }

    let scale_x = app.camera.scale_x;
    let scale_y = app.camera.scale_y;
    let trans_x = app.camera.translate_x;
    let trans_y = app.camera.translate_y;

    // egui paints in points; tap/camera math is in physical pixels.
    let ppp = ctx.pixels_per_point();
    let screen = ctx.screen_rect();
    let w_px = screen.width() * ppp;
    let h_px = screen.height() * ppp;

    let drawables = model.drawables();
    let ids = drawables.ids();
    let vpos = drawables.vertex_positions();
    let counts = drawables.vertex_counts();

    // Model-space AABB → NDC → physical px → egui points (same transform
    // family as handle_tap_with_cam, inverted).
    let mut rects: Vec<(egui::Rect, String)> = Vec::new();
    for area in &app.hit_areas {
        let Some(di) = ids.iter().position(|id| id.to_string_lossy() == area.id) else {
            continue;
        };
        if di >= counts.len() {
            continue;
        }
        let n = counts[di] as usize;
        if n == 0 || di >= vpos.len() {
            continue;
        }
        let verts = unsafe { std::slice::from_raw_parts(vpos[di], n) };

        let (mut min_x, mut min_y) = (f32::INFINITY, f32::INFINITY);
        let (mut max_x, mut max_y) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
        for v in verts {
            min_x = min_x.min(v.X);
            max_x = max_x.max(v.X);
            min_y = min_y.min(v.Y);
            max_y = max_y.max(v.Y);
        }

        let corners = [
            (min_x, min_y),
            (max_x, min_y),
            (max_x, max_y),
            (min_x, max_y),
        ];
        let mut pts = [egui::pos2(0.0, 0.0); 4];
        for (pt, (mx, my)) in pts.iter_mut().zip(corners.iter()) {
            let ndc_x = mx * scale_x + trans_x;
            let ndc_y = my * scale_y + trans_y;
            let px = (ndc_x + 1.0) * 0.5 * w_px;
            let py = (1.0 - ndc_y) * 0.5 * h_px;
            *pt = egui::pos2(px / ppp, py / ppp);
        }
        let rect = egui::Rect::from_min_max(pts[0].min(pts[2]), pts[0].max(pts[2]));
        rects.push((rect, area.name.clone()));
    }

    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("hit_area_debug_overlay"),
    ));
    let stroke = egui::Stroke::new(1.5_f32, egui::Color32::YELLOW);
    for (rect, label) in rects {
        painter.rect_stroke(rect, 0.0, stroke);
        painter.text(
            rect.left_top(),
            egui::Align2::LEFT_BOTTOM,
            &label,
            egui::FontId::proportional(12.0),
            egui::Color32::YELLOW,
        );
    }
}

fn draw_settings(ctx: &Context, app: &mut AppState) {
    let mut changed = false;
    Window::new("Settings")
        .default_width(400.0)
        .show(ctx, |ui| {
            ui.heading("Model Scan Directories");
            ui.label("Recursively scanned for V2/V3 model folders:");
            ui.separator();

            let mut remove_idx = None;
            {
                let dirs: Vec<PathBuf> = app.scan_dirs.clone();
                for (i, d) in dirs.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(d.to_string_lossy());
                        let rm_btn = theme::button(ui, theme::ButtonVariant::Danger, "✖");
                        if ui.add(rm_btn).clicked() {
                            remove_idx = Some(i);
                        }
                    });
                }
            }
            if let Some(i) = remove_idx {
                app.scan_dirs.remove(i);
                changed = true;
            }

            let add_dir_btn = theme::button(ui, theme::ButtonVariant::Primary, "+ Add Directory");
            if ui.add(add_dir_btn).clicked() {
                if let Some(path) = rfd::FileDialog::new().pick_folder() {
                    let s = path.to_string_lossy().to_string();
                    if !app.scan_dirs.iter().any(|d| d.to_string_lossy() == s) {
                        app.scan_dirs.push(path);
                        changed = true;
                    }
                }
            }

            ui.separator();
            let scan_btn = theme::button(ui, theme::ButtonVariant::Primary, "\u{1f50d} Scan All");
            if ui.add(scan_btn).clicked() {
                let (added, skipped, invalid) = app.scan_and_add_models();
                app.scan_result = format!("{added} new, {skipped} skipped, {invalid} invalid");
            }
            if !app.scan_result.is_empty() {
                ui.label(format!("Result: {}", app.scan_result));
            }

            ui.separator();
            ui.heading("AI Chat");
            let ai_btn = theme::button(ui, theme::ButtonVariant::Secondary, "AI 设置");
            if ui.add(ai_btn).clicked() {
                app.ai_settings_open = !app.ai_settings_open;
            }
            if !app.ai_enabled {
                ui.colored_label(egui::Color32::GRAY, "AI not enabled");
            }

            // ── Theme settings ──
            ui.separator();
            ui.heading("Theme");
            let mut dark = app.theme_dark;
            if ui
                .add(egui::Checkbox::new(&mut dark, "Dark mode"))
                .changed()
            {
                app.theme_dark = dark;
                app.theme_needs_refresh = true;
            }

            ui.separator();
            let mut dbg_hit = app.debug_hit_areas;
            if ui
                .add(egui::Checkbox::new(&mut dbg_hit, "HitArea 调试框(黄框)"))
                .changed()
            {
                app.debug_hit_areas = dbg_hit;
            }
            ui.horizontal(|ui| {
                ui.label("Preset:");
                let presets = ["AIRA", "Coral", "Mint", "Purple"];
                for (i, name) in presets.iter().enumerate() {
                    if ui
                        .add(egui::SelectableLabel::new(app.theme_preset == i, *name))
                        .clicked()
                    {
                        app.theme_preset = i;
                        app.theme_needs_refresh = true;
                    }
                }
            });
        });
    if changed {
        app.save_scan_dirs();
    }

    crate::ai::settings_panel::draw_settings_panel(ctx, app);
}

fn small_btn(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(egui::Button::new(label).min_size(egui::vec2(24.0, 22.0)))
}

#[cfg(feature = "capture")]
fn draw_capture_preview(ctx: &egui::Context, app: &mut AppState) {
    if !app.capture_window_open {
        return;
    }

    let default_size = if let Some(ref frame) = app.capture_latest_frame {
        let screen = ctx.screen_rect();
        let (sw, sh) = (screen.width(), screen.height());
        let (cw, ch) = (frame.width as f32, frame.height as f32);
        if cw > 0.0 && ch > 0.0 {
            let scale = (sw / cw).max(sh / ch) * 0.25;
            egui::vec2((cw * scale).max(160.0), (ch * scale).max(120.0))
        } else {
            egui::vec2(320.0, 240.0)
        }
    } else {
        egui::vec2(320.0, 240.0)
    };

    egui::Window::new("Capture Preview")
        .id("capture_preview_window".into())
        .resizable(true)
        .collapsible(true)
        .default_size(default_size)
        .show(ctx, |ui| {
            if let Some(ref tex) = app.capture_texture {
                let tex_id = tex.id();
                let img_size = egui::vec2(tex.size()[0] as f32, tex.size()[1] as f32);
                let available = ui.available_size();
                let scale = (available.x / img_size.x)
                    .min(available.y / img_size.y)
                    .min(1.0);
                let display_size = egui::vec2(img_size.x * scale, img_size.y * scale);
                ui.add(egui::Image::new((tex_id, display_size)));
                ui.label(format!(
                    "{}x{} · F9 to stop",
                    app.capture_latest_frame
                        .as_ref()
                        .map(|f| f.width)
                        .unwrap_or(0),
                    app.capture_latest_frame
                        .as_ref()
                        .map(|f| f.height)
                        .unwrap_or(0)
                ));
            } else {
                ui.label("Waiting for capture data...");
                ui.label("F9 to start capture");
            }
        });
}
