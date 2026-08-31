use crate::app::AppState;
use egui::Window;

/// Draw the AI settings panel (provider config + TTS config).
pub fn draw_settings_panel(ctx: &egui::Context, app: &mut AppState) {
    if !app.ai_settings_open {
        app.settings_panel_was_open = false;
        return;
    }

    // Sync input buffers from config when panel first opens
    if !app.settings_panel_was_open {
        app.tool_calling_cmds_input = app.ai_config.allowed_commands.join(", ");
        app.tool_calling_paths_input = app.ai_config.allowed_read_paths.join(", ");
    }
    app.settings_panel_was_open = true;

    Window::new("AI 设置")
        .default_width(360.0)
        .default_height(500.0)
        .resizable(true)
        .open(&mut app.ai_settings_open)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .max_height(460.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let dirty = &mut false;

                    ui.label("API 基础地址");
                    ui.text_edit_singleline(&mut app.ai_config.base_url);

                    ui.label("API Key");
                    ui.text_edit_singleline(&mut app.ai_config.api_key);

                    ui.label("模型");
                    ui.text_edit_singleline(&mut app.ai_config.model);

                    ui.add_space(4.0);
                    if ui.button("📝 编辑角色卡").clicked() {
                        app.character_card_editor_open = true;
                    }

                    ui.add_space(8.0);

                    ui.label("上下文长度");
                    if ui
                        .add(
                            egui::DragValue::new(&mut app.ai_config.context_length)
                                .clamp_range(1..=100),
                        )
                        .changed()
                    {
                        *dirty = true;
                    }

                    ui.add_space(8.0);

                    ui.label("系统提示词 / 角色设定");
                    ui.text_edit_multiline(&mut app.ai_config.system_prompt);

                    ui.add_space(12.0);

                    // Test connection button
                    if ui.button("测试连接").clicked() {
                        let client = crate::ai::client::AiChatClient::new();
                        match client.test_connection(&app.ai_config) {
                            Ok(msg) => {
                                app.ai_test_result = Some((msg, true));
                            }
                            Err(e) => {
                                app.ai_test_result = Some((e, false));
                            }
                        }
                    }
                    if let Some((ref msg, ok)) = app.ai_test_result {
                        let color = if ok {
                            egui::Color32::GREEN
                        } else {
                            egui::Color32::RED
                        };
                        ui.colored_label(color, msg);
                        if ui.button("清除").clicked() {
                            app.ai_test_result = None;
                        }
                    }

                    ui.add_space(16.0);
                    ui.separator();
                    ui.heading("语音合成 (TTS)");

                    ui.checkbox(&mut app.ai_config.tts_enabled, "启用语音合成");

                    if app.ai_config.tts_enabled {
                        ui.add_space(4.0);
                        ui.label("TTS API 地址");
                        ui.text_edit_singleline(&mut app.ai_config.tts_api_url);

                        ui.label("TTS Key");
                        ui.text_edit_singleline(&mut app.ai_config.tts_key);

                        ui.add_space(4.0);
                        if ui.button("刷新音色列表").clicked() {
                            app.tts_refresh_requested = true;
                        }

                        if app.tts_voices_cache.is_empty() {
                            ui.colored_label(
                                egui::Color32::GRAY,
                                "点击刷新获取可用音色列表（需先填写 TTS Key）",
                            );
                        } else {
                            let current = &app.ai_config.tts_voice;
                            let mut selected = current.clone();
                            egui::ComboBox::from_id_source("tts_voice_selector")
                                .selected_text(
                                    app.tts_voices_cache
                                        .iter()
                                        .find(|v| v.voice_id == *current)
                                        .map(|v| format!("{} ({})", v.voice_name, v.gender))
                                        .unwrap_or_else(|| current.clone()),
                                )
                                .show_ui(ui, |ui| {
                                    for voice in &app.tts_voices_cache {
                                        let label = format!(
                                            "{} - {} ({}) [{}]",
                                            voice.voice_name,
                                            voice.voice_id,
                                            voice.gender,
                                            voice.language
                                        );
                                        ui.selectable_value(
                                            &mut selected,
                                            voice.voice_id.clone(),
                                            label,
                                        );
                                    }
                                });
                            if selected != *current {
                                app.ai_config.tts_voice = selected;
                                *dirty = true;
                            }
                        }
                    }

                    ui.add_space(16.0);
                    ui.separator();
                    ui.heading("工具调用 (Tool Calling)");

                    ui.checkbox(
                        &mut app.ai_config.tool_calling_enabled,
                        "启用工具调用（AI 可执行命令、读文件等）",
                    );

                    if app.ai_config.tool_calling_enabled {
                        ui.add_space(4.0);
                        ui.label("每轮最大工具调用次数");
                        if ui
                            .add(
                                egui::DragValue::new(&mut app.ai_config.max_tool_rounds)
                                    .clamp_range(1..=50),
                            )
                            .changed()
                        {
                            *dirty = true;
                        }

                        ui.add_space(4.0);
                        ui.label("允许的命令（逗号分隔，留空=全部需审批）");
                        let mut cmds = app.tool_calling_cmds_input.clone();
                        let cmds_resp = ui.text_edit_singleline(&mut cmds);
                        if cmds_resp.changed() {
                            app.tool_calling_cmds_input = cmds;
                        }
                        if cmds_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            // parse on Enter
                        }
                        if cmds_resp.lost_focus() {
                            app.ai_config.allowed_commands = app
                                .tool_calling_cmds_input
                                .split(',')
                                .map(|s| s.trim().to_string())
                                .filter(|s| !s.is_empty())
                                .collect();
                            *dirty = true;
                        }

                        ui.add_space(4.0);
                        ui.label("允许读取的路径前缀（逗号分隔，留空=不限制）");
                        let mut paths = app.tool_calling_paths_input.clone();
                        let paths_resp = ui.text_edit_singleline(&mut paths);
                        if paths_resp.changed() {
                            app.tool_calling_paths_input = paths;
                        }
                        if paths_resp.lost_focus() {
                            app.ai_config.allowed_read_paths = app
                                .tool_calling_paths_input
                                .split(',')
                                .map(|s| s.trim().to_string())
                                .filter(|s| !s.is_empty())
                                .collect();
                            *dirty = true;
                        }
                    }

                    #[cfg(feature = "capture")]
                    {
                        ui.add_space(16.0);
                        ui.separator();
                        ui.heading("视觉 (Vision)");

                        ui.checkbox(
                            &mut app.ai_config.vision_auto_enabled,
                            "自动观察屏幕（定时截图发给 AI 分析）",
                        );

                        if app.ai_config.vision_auto_enabled {
                            ui.add_space(4.0);
                            ui.label("观察间隔（秒）");
                            let interval_changed = ui
                                .add(
                                    egui::DragValue::new(&mut app.ai_config.vision_interval_secs)
                                        .clamp_range(10..=600),
                                )
                                .changed();
                            if interval_changed {
                                *dirty = true;
                            }
                        }

                        ui.add_space(4.0);
                        ui.label("本地视觉模型 (GGUF)");
                        ui.label("留空则使用 API 视觉模型");
                        let path_changed = ui
                            .text_edit_singleline(&mut app.ai_config.vision_gguf_path)
                            .changed();
                        if path_changed {
                            *dirty = true;
                        }
                        ui.label("mmproj 路径（多模态投影，为空则跳过）");
                        let mmproj_changed = ui
                            .text_edit_singleline(&mut app.ai_config.vision_mmproj_path)
                            .changed();
                        if mmproj_changed {
                            *dirty = true;
                        }
                    }

                    ui.add_space(8.0);

                    // Save button — write to DB + JSON file
                    if ui.button("保存设置").clicked() {
                        crate::ai::config::save_config(&app.ai_config, app.db.as_deref());
                    }
                }); // end ScrollArea
        });
}
