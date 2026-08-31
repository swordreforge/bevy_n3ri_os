use pulldown_cmark::{Event, Options, Parser, Tag};

use crate::ai::types::{AiState, ChatRole};
use crate::app::AppState;
use egui::{Color32, Grid, Window};

/// Strip `<system-reminder>...</system-reminder>` and similar HTML tags from content.
fn strip_system_tags(text: &str) -> String {
    let mut result = text.to_string();
    // Strip <system-reminder>...</system-reminder> blocks (case-insensitive)
    while let Some(start) = result.to_lowercase().find("<system-reminder") {
        if let Some(tag_end) = result[start..].find('>') {
            let content_start = start + tag_end + 1;
            if let Some(end) = result[content_start..]
                .to_lowercase()
                .find("</system-reminder>")
            {
                let content_end = content_start + end;
                result = format!(
                    "{}{}",
                    &result[..start],
                    &result[content_end + "</system-reminder>".len()..]
                );
            } else {
                // No closing tag — strip from opening tag to end
                result = result[..start].to_string();
                break;
            }
        } else {
            break;
        }
    }
    result
}

fn flush_plain(ui: &mut egui::Ui, buf: &mut String, bold: bool) {
    if !buf.is_empty() {
        let text = std::mem::take(buf);
        if bold {
            ui.label(egui::RichText::new(text).strong());
        } else {
            ui.label(text);
        }
    }
}

/// Render a markdown string into egui widgets.
///
/// Handles paragraphs, bold, inline code, code blocks, headings, lists,
/// rules, tables — the common subset AI chat responses typically use.
fn render_markdown(ui: &mut egui::Ui, text: &str) {
    let parser = Parser::new_ext(text, Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH);
    let mut bold: u32 = 0;
    let mut plain = String::new();

    // Table accumulation state
    enum TableCellKind {
        Header,
        Body,
    }
    let mut in_table = false;
    let mut table_cols: usize = 0;
    let mut table_rows: Vec<(TableCellKind, Vec<String>)> = Vec::new();
    let mut table_cur_row: Vec<String> = Vec::new();
    let mut table_cur_cell = String::new();
    let mut table_in_head = false;
    let mut table_in_cell = false;

    for event in parser {
        match event {
            // --- text-like events ---
            Event::Text(t) => {
                if in_table && table_in_cell {
                    table_cur_cell.push_str(&t);
                } else {
                    plain.push_str(&t);
                }
            }
            Event::Code(t) => {
                if in_table && table_in_cell {
                    table_cur_cell.push_str(&format!(" `{}` ", t));
                } else {
                    flush_plain(ui, &mut plain, bold > 0);
                    ui.colored_label(Color32::from_rgb(230, 200, 150), format!(" `{}` ", t));
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if in_table && table_in_cell {
                    table_cur_cell.push(' ');
                } else {
                    flush_plain(ui, &mut plain, bold > 0);
                }
            }
            Event::Html(t) => {
                if in_table && table_in_cell {
                    table_cur_cell.push_str(&t);
                } else {
                    flush_plain(ui, &mut plain, bold > 0);
                    ui.label(t.as_ref());
                }
            }

            // --- Start tags ---
            Event::Start(tag) => match tag {
                Tag::Table(alignments) => {
                    flush_plain(ui, &mut plain, bold > 0);
                    in_table = true;
                    table_cols = alignments.len();
                    table_rows.clear();
                    table_cur_row = Vec::new();
                    table_cur_cell.clear();
                    table_in_head = false;
                    table_in_cell = false;
                }
                Tag::TableHead => {
                    table_in_head = true;
                }
                Tag::TableRow => {
                    table_cur_row = Vec::new();
                }
                Tag::TableCell => {
                    table_in_cell = true;
                    table_cur_cell.clear();
                }
                Tag::Paragraph => {}
                Tag::CodeBlock(_kind) => {
                    flush_plain(ui, &mut plain, bold > 0);
                }
                Tag::Emphasis | Tag::Strong => {
                    flush_plain(ui, &mut plain, bold > 0);
                    bold += 1;
                }
                Tag::Strikethrough => {
                    flush_plain(ui, &mut plain, bold > 0);
                }
                Tag::Heading(..) => {
                    flush_plain(ui, &mut plain, bold > 0);
                    ui.add_space(4.0);
                }
                Tag::List(_) => {
                    flush_plain(ui, &mut plain, bold > 0);
                }
                Tag::Item => {
                    flush_plain(ui, &mut plain, bold > 0);
                }
                _ => {}
            },

            // --- End tags ---
            Event::End(tag) => match tag {
                Tag::Table(_) => {
                    // Flush final row if non-empty
                    if !table_cur_row.is_empty() || !table_cur_cell.is_empty() {
                        if table_in_cell {
                            // last cell wasn't closed
                        }
                        if !table_cur_row.is_empty() || !table_cur_cell.is_empty() {
                            let kind = if table_in_head {
                                TableCellKind::Header
                            } else {
                                TableCellKind::Body
                            };
                            table_rows.push((kind, table_cur_row.clone()));
                        }
                    }

                    // Render the table
                    if !table_rows.is_empty() {
                        let id = ui.next_auto_id();
                        Grid::new(id)
                            .striped(true)
                            .min_col_width(40.0)
                            .show(ui, |ui| {
                                for (kind, cells) in &table_rows {
                                    let is_header = matches!(kind, TableCellKind::Header);
                                    for cell in cells {
                                        if is_header {
                                            ui.label(
                                                egui::RichText::new(cell)
                                                    .strong()
                                                    .color(Color32::from_rgb(180, 210, 255)),
                                            );
                                        } else {
                                            ui.label(cell.as_str());
                                        }
                                    }
                                    ui.end_row();
                                }
                            });
                    }

                    in_table = false;
                    table_cur_cell.clear();
                    table_cur_row.clear();
                    table_rows.clear();
                }
                Tag::TableHead => {
                    table_in_head = false;
                }
                Tag::TableRow => {
                    if !table_cur_row.is_empty() || !table_cur_cell.is_empty() {
                        // Push the pending cell if any
                        if table_in_cell {
                            table_cur_row.push(std::mem::take(&mut table_cur_cell));
                            table_in_cell = false;
                        }
                        // Pad row to table_cols
                        while table_cur_row.len() < table_cols {
                            table_cur_row.push(String::new());
                        }
                        let kind = if table_in_head {
                            TableCellKind::Header
                        } else {
                            TableCellKind::Body
                        };
                        table_rows.push((kind, std::mem::take(&mut table_cur_row)));
                    }
                }
                Tag::TableCell => {
                    if table_in_cell {
                        table_cur_row.push(std::mem::take(&mut table_cur_cell));
                        table_in_cell = false;
                    }
                }
                Tag::Paragraph => {
                    flush_plain(ui, &mut plain, bold > 0);
                }
                Tag::CodeBlock(_kind) => {}
                Tag::Emphasis | Tag::Strong => {
                    flush_plain(ui, &mut plain, bold > 0);
                    bold = bold.saturating_sub(1);
                }
                Tag::Strikethrough => {
                    flush_plain(ui, &mut plain, bold > 0);
                }
                Tag::Heading(..) => {
                    flush_plain(ui, &mut plain, bold > 0);
                    ui.add_space(4.0);
                }
                Tag::List(_) => {}
                Tag::Item => {
                    flush_plain(ui, &mut plain, bold > 0);
                }
                _ => {}
            },

            Event::Rule => {
                flush_plain(ui, &mut plain, bold > 0);
                ui.separator();
            }
            _ => {}
        }
    }
    flush_plain(ui, &mut plain, bold > 0);
}

/// Draw the AI chat panel inside a normal-mode egui window.
pub fn draw_chat_panel(ctx: &egui::Context, app: &mut AppState) {
    if !app.ai_chat_open {
        return;
    }

    let enter_triggered =
        app.ai_chat_open && ctx.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift);
    let input_before = app.ai_input_buffer.clone();
    let pending = !matches!(app.ai_state, AiState::Idle);

    // Extract pending tool info (cloned) before entering the UI closure to avoid borrow conflicts.
    let pending_tool_info = match &app.ai_state {
        AiState::PendingTool {
            tool_name, args, ..
        } => Some((
            tool_name.clone(),
            serde_json::to_string_pretty(args).unwrap_or_default(),
        )),
        _ => None,
    };

    let mut clicked_send = false;
    let mut approve_clicked = false;
    let mut reject_clicked = false;
    let mut remember_clicked = false;

    // Extract card name before Window borrows app
    let current_path = app
        .current_model_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let card_name = app
        .character_cards
        .get(&current_path)
        .map(|c| c.name.clone())
        .unwrap_or_default();

    Window::new("AI 聊天 💬")
        .default_width(320.0)
        .default_height(300.0)
        .min_height(300.0)
        .default_pos([4.0, 100.0])
        .resizable(true)
        .scroll2([false, true])
        .open(&mut app.ai_chat_open)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("模型: {}", app.ai_config.model))
                        .text_style(egui::TextStyle::Small),
                );
                if !card_name.is_empty() {
                    ui.separator();
                    ui.label(
                        egui::RichText::new(format!("角色: {card_name}"))
                            .color(Color32::LIGHT_GREEN),
                    );
                }
                if ui.button("📝").on_hover_text("编辑角色卡").clicked() {
                    app.character_card_editor_open = true;
                }
            });
            ui.separator();

            for msg in &app.ai_messages {
                let (prefix, color) = match msg.role {
                    ChatRole::User => ("你", Color32::LIGHT_BLUE),
                    ChatRole::Assistant => ("AI", Color32::LIGHT_GREEN),
                    ChatRole::System => ("系统", Color32::GRAY),
                    ChatRole::Tool => ("工具", Color32::YELLOW),
                };
                ui.colored_label(color, format!("[{}]", prefix));

                let cleaned = strip_system_tags(&msg.content);
                if msg.role == ChatRole::Assistant {
                    render_markdown(ui, &cleaned);
                } else {
                    let _ = ui.selectable_label(false, &cleaned);
                }
                if ui.small_button("\u{1F4CB}").on_hover_text("复制").clicked() {
                    ui.ctx().copy_text(cleaned.clone());
                }
                ui.add_space(4.0);
            }

            // ── Tool approval UI ──
            if let Some((ref tool_name, ref args_str)) = pending_tool_info {
                let frame = egui::Frame::none()
                    .fill(Color32::from_rgba_premultiplied(40, 0, 0, 200))
                    .inner_margin(egui::Margin::symmetric(8.0, 4.0));
                frame.show(ui, |ui| {
                    ui.colored_label(Color32::RED, "⚠️ 工具调用需要审批");
                    ui.label(format!("工具: {tool_name}"));
                    ui.label(format!("参数: {args_str}"));
                    ui.horizontal(|ui| {
                        if ui.button("✅ 批准").clicked() {
                            approve_clicked = true;
                        }
                        if ui.button("❌ 拒绝").clicked() {
                            reject_clicked = true;
                        }
                    });
                    if ui.button("📌 记住本次会话（自动批准此工具）").clicked() {
                        remember_clicked = true;
                    }
                });
            }

            if pending {
                ui.colored_label(Color32::YELLOW, "思考中...");
            }

            if let Some(ref err) = app.ai_error {
                ui.colored_label(Color32::RED, err);
                if ui.button("清除错误").clicked() {
                    app.ai_error = None;
                }
            }

            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.add_sized(
                    egui::vec2(200.0, 0.0),
                    egui::TextEdit::singleline(&mut app.ai_input_buffer).hint_text("输入消息..."),
                );
                if ui
                    .add_enabled(!pending, egui::Button::new("发送"))
                    .clicked()
                {
                    clicked_send = true;
                }
            });
        });

    if !pending && (clicked_send || (enter_triggered && !input_before.trim().is_empty())) {
        app.send_ai_message();
    }
    if approve_clicked {
        app.approve_tool();
    }
    if reject_clicked {
        app.reject_tool();
    }
    if remember_clicked {
        if let AiState::PendingTool { ref tool_name, .. } = app.ai_state {
            let name = tool_name.clone();
            app.session_approved_tools.insert(name);
            app.approve_tool();
        }
    }
}
