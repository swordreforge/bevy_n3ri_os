use crate::ai::types::CharacterCard;
use crate::app::AppState;
use egui::Window;

/// Draw the character card editor panel for the current model.
///
/// Displays an egui `Window` titled "角色卡编辑器" with per-model AI
/// companion profile fields (name, description, personality, scenario,
/// example dialogs, system prompt, TTS voice). Edits are applied to a
/// local copy from the in-memory cache; Save writes to SQLite and
/// updates the cache, Delete removes from both.
pub fn draw_character_card_panel(ctx: &egui::Context, app: &mut AppState) {
    if !app.character_card_editor_open {
        return;
    }

    // ── Extract everything from `app` before entering the Window closure ──
    let (file_path, model_name) = match app.current_model_dir() {
        Some(p) => {
            let fp = p.to_string_lossy().to_string();
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| fp.clone());
            (fp, name)
        }
        None => (String::new(), "未选择模型".to_string()),
    };

    let has_model = !file_path.is_empty();
    let db_exists = app.db.is_some();

    let mut card = if has_model && db_exists {
        app.character_cards
            .get(&file_path)
            .cloned()
            .unwrap_or_else(|| app.load_character_card(&file_path))
    } else {
        CharacterCard::default()
    };

    // Clone the TTS voice cache so we can reference it inside the closure.
    let tts_voices = app.tts_voices_cache.clone();
    // Borrow `db` before the closure (Option<&AppDb>).
    let db = app.db.as_ref();

    let mut open = true;
    let mut save_requested = false;
    let mut delete_requested = false;

    // ── Window ──
    Window::new("角色卡编辑器")
        .default_width(400.0)
        .default_height(420.0)
        .default_pos([100.0, 100.0])
        .scroll2([false, true])
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label(format!("模型: {model_name}"));
            ui.separator();

            // Edge case: no model
            if !has_model {
                ui.label("请先加载模型");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.add_enabled(false, egui::Button::new("保存"));
                    ui.add_enabled(false, egui::Button::new("删除"));
                });
                return;
            }

            // Edge case: no database
            if !db_exists {
                ui.label("数据库不可用");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.add_enabled(false, egui::Button::new("保存"));
                    ui.add_enabled(false, egui::Button::new("删除"));
                });
                return;
            }

            // ── Fields ──
            ui.label("角色名");
            ui.text_edit_singleline(&mut card.name);

            ui.add_space(4.0);

            ui.label("描述（背景故事）");
            ui.add(egui::TextEdit::multiline(&mut card.description).desired_rows(6));

            ui.add_space(4.0);

            ui.label("性格");
            ui.add(egui::TextEdit::multiline(&mut card.personality).desired_rows(4));

            ui.add_space(4.0);

            ui.label("场景（世界观）");
            ui.add(egui::TextEdit::multiline(&mut card.scenario).desired_rows(4));

            ui.add_space(4.0);

            ui.label("对话示例");
            ui.add(egui::TextEdit::multiline(&mut card.example_dialogs).desired_rows(6));

            ui.add_space(4.0);

            ui.label("系统提示词（补充）");
            ui.add(egui::TextEdit::multiline(&mut card.system_prompt).desired_rows(4));

            ui.add_space(4.0);

            // ── TTS voice selector ──
            ui.label("TTS 语音");
            ui.horizontal(|ui| {
                let current_voice = card.tts_voice.clone();
                let mut selected_voice = current_voice.clone();
                egui::ComboBox::from_id_source("char_card_tts_voice")
                    .selected_text(if current_voice.is_empty() {
                        "使用全局默认".to_string()
                    } else {
                        tts_voices
                            .iter()
                            .find(|v| v.voice_id == current_voice)
                            .map(|v| format!("{} ({})", v.voice_name, v.gender))
                            .unwrap_or_else(|| current_voice.clone())
                    })
                    .show_ui(ui, |ui| {
                        for voice in &tts_voices {
                            let label = format!(
                                "{} - {} ({}) [{}]",
                                voice.voice_name, voice.voice_id, voice.gender, voice.language
                            );
                            ui.selectable_value(&mut selected_voice, voice.voice_id.clone(), label);
                        }
                    });
                if selected_voice != current_voice {
                    card.tts_voice = selected_voice;
                }
                if ui.button("使用全局默认").clicked() {
                    card.tts_voice.clear();
                }
            });

            ui.add_space(12.0);

            // ── Save / Delete buttons ──
            ui.horizontal(|ui| {
                if ui.button("保存").clicked() {
                    card.file_path.clone_from(&file_path);
                    save_requested = true;
                }

                if ui.button("删除").clicked() {
                    delete_requested = true;
                }
            });
        });

    // ── Post-window: sync back to AppState ──
    app.character_card_editor_open = open;

    if save_requested {
        if let Some(db) = db {
            let _ = db.save_character_card(&card);
        }
        app.character_cards.insert(file_path.clone(), card);
    }

    if delete_requested {
        if let Some(db) = db {
            let _ = db.delete_character_card(&file_path);
        }
        app.character_cards.remove(&file_path);
    }
}
