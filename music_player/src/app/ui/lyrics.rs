//! Floating lyrics window and the drag-and-drop hint overlay.

use crate::app::App;
use crate::lang::Lang;
use eframe::egui;
use egui::{Color32, FontId};

impl App {
    /// Draws the floating lyrics window, highlighting the line for the current
    /// playback position.
    pub(in crate::app) fn ui_lyrics_window(&mut self, ctx: &egui::Context) {
        egui::Window::new("🎤 Текст песни BETA!!").show(ctx, |ui| {
            let Some(lyrics) = &self.current_lyrics else {
                ui.label("Текст песни отсутствует 😔");
                return;
            };

            egui::ScrollArea::vertical().show(ui, |ui| {
                for (index, line) in lyrics.iter().enumerate() {
                    // A line is "active" once playback has passed its timestamp
                    // but not yet reached the next line's.
                    let is_active = {
                        let is_past_start = self.current_playback_time_ms >= line.time_ms;
                        let is_before_next = match lyrics.get(index + 1) {
                            Some(next_line) => self.current_playback_time_ms < next_line.time_ms,
                            None => true,
                        };
                        is_past_start && is_before_next
                    };

                    let (color, font_size) = if is_active {
                        (Color32::WHITE, 24.0)
                    } else {
                        (Color32::GRAY, 18.0)
                    };

                    ui.label(egui::RichText::new(&line.text).color(color).size(font_size));
                }
            });
        });
    }

    /// Draws a full-screen hint while the user is dragging files over the window.
    pub(in crate::app) fn ui_drop_hint(&mut self, ctx: &egui::Context) {
        let hovering_files = ctx.input(|i| !i.raw.hovered_files.is_empty());
        if !hovering_files {
            return;
        }

        let screen = ctx.screen_rect();
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("drop_files_overlay"),
        ));
        painter.rect_filled(screen, egui::Rounding::same(0.0), Color32::from_black_alpha(150));
        let text = match self.language {
            Lang::Ru => "Отпустите, чтобы добавить в Elysium",
            Lang::Uk => "Відпустіть, щоб додати в Elysium",
            Lang::En => "Drop to add to Elysium",
        };
        painter.text(
            screen.center(),
            egui::Align2::CENTER_CENTER,
            text,
            FontId::proportional(28.0),
            Color32::WHITE,
        );
    }
}
