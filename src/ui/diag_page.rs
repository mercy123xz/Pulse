//! Diagnostics: versions, a self-test, where disk space goes and recent problems,
//! with one button that copies it all for a bug report.

use super::page_title;
use crate::app::App;
use crate::diag::{Check, SERVICES};
use crate::i18n::tr;
use crate::settings::fmt_bytes;
use crate::theme::*;
use crate::widgets::*;
use egui::{Align, Color32, Layout, RichText, Ui};
use egui_phosphor::regular as icon;

const GOOD: Color32 = Color32::from_rgb(46, 160, 67);
const BAD: Color32 = Color32::from_rgb(229, 83, 75);

pub fn diagnostics_page(app: &mut App, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let offline = app.saved.settings.offline_dir();
    app.diag.poll(&ctx, offline.clone());
    page_title(ui, "Diagnostics", Some("For when something isn't working. Nothing here leaves your PC unless you copy it."));
    ui.set_max_width(ui.available_width().min(860.0));
    ui.add_space(6.0);

    ui.horizontal_wrapped(|ui| {
        let copy = egui::Button::new(RichText::new(format!("{}  {}", icon::COPY, tr("Copy report"))).font(med(13.0)).color(Color32::WHITE))
            .fill(pal().accent)
            .corner_radius(17)
            .min_size(egui::vec2(0.0, 34.0));
        if ui.add(copy).clicked() {
            ctx.copy_text(crate::diag::report(&app.diag, &app.saved.settings));
            app.toast(tr("Report copied"), &ctx);
        }
        ui.add_enabled_ui(!app.diag.testing(), |ui| {
            if ghost_button(ui, icon::PULSE, "Run self-test").clicked() {
                app.diag.run_self_test(&ctx);
            }
        });
        ui.add_enabled_ui(!app.diag.loading(), |ui| {
            if ghost_button(ui, icon::ARROWS_CLOCKWISE, "Refresh").clicked() {
                app.diag.refresh(&ctx, offline.clone());
            }
        });
    });

    // ------------------------------------------------------------ Versions
    heading(ui, tr("About this PC"));
    let facts = app.diag.facts.clone();
    let waiting = tr("Checking…");
    let gpu = app.diag.gpu.clone().unwrap_or_else(|| waiting.to_string());
    let os = facts.as_ref().map_or(waiting, |f| f.os.as_str()).to_string();
    let ytdlp = facts.as_ref().map_or(waiting, |f| f.ytdlp.as_str()).to_string();
    let deno = facts.as_ref().map_or(waiting, |f| f.deno.as_str()).to_string();
    kv(ui, "Pulse", env!("CARGO_PKG_VERSION"));
    kv(ui, tr("System"), &os);
    kv(ui, tr("Graphics"), &gpu);
    kv(ui, "yt-dlp", &ytdlp);
    kv(ui, "Deno", &deno);

    // ------------------------------------------------------------ Self-test
    heading(ui, tr("Self-test"));
    ui.add(egui::Label::new(RichText::new(tr("Checks that Pulse can reach the services it uses. It only asks each one for something small.")).color(pal().text_muted)).wrap());
    ui.add_space(8.0);
    for (i, service) in SERVICES.iter().enumerate() {
        let check = app.diag.checks.get(i).cloned();
        ui.horizontal(|ui| {
            match &check {
                None => {
                    ui.label(RichText::new(icon::CIRCLE).size(16.0).color(pal().text_faint));
                }
                Some(Check::Running) => {
                    ui.add(egui::Spinner::new().size(15.0).color(pal().text_muted));
                }
                Some(Check::Passed { .. }) => {
                    ui.label(RichText::new(icon::CHECK_CIRCLE).size(16.0).color(GOOD));
                }
                Some(Check::Failed(_)) => {
                    ui.label(RichText::new(icon::X_CIRCLE).size(16.0).color(BAD));
                }
            }
            let used = ui.min_rect().width() + ui.spacing().item_spacing.x;
            fixed(ui, COLUMN - used, egui::Label::new(RichText::new(tr(service.name)).font(med(13.5)).color(pal().text)));
            let text = check.as_ref().map_or_else(|| tr("Not run yet").to_string(), Check::summary);
            let color = if matches!(check, Some(Check::Failed(_))) { BAD } else { pal().text_muted };
            ui.add(egui::Label::new(RichText::new(text).font(regular(13.0)).color(color)).wrap());
        });
        ui.add_space(4.0);
    }

    // ------------------------------------------------------------ Storage
    heading(ui, tr("Storage"));
    match &facts {
        None => skeleton_rows(ui, 4),
        Some(f) => {
            for (name, n) in &f.storage {
                kv(ui, tr(name), &fmt_bytes(*n));
            }
            let total: u64 = f.storage.iter().map(|(_, n)| n).sum();
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                fixed(ui, COLUMN, egui::Label::new(RichText::new(tr("Total")).font(heavy(13.5)).color(pal().text)));
                ui.label(RichText::new(fmt_bytes(total)).font(heavy(13.5)).color(pal().text));
            });
        }
    }
    ui.add_space(10.0);
    ui.horizontal_wrapped(|ui| {
        if ghost_button(ui, icon::FOLDER_OPEN, "Open data folder").clicked() {
            ctx.open_url(egui::OpenUrl::new_tab(crate::settings::file_uri(&crate::settings::data_dir())));
        }
        ui.label(RichText::new(tr("Clear the cache from Settings › Storage.")).font(regular(12.5)).color(pal().text_faint));
    });

    // ------------------------------------------------------------ Problems
    heading(ui, tr("Recent problems"));
    let problems = crate::crash::recent();
    if problems.is_empty() {
        ui.label(RichText::new(tr("No problems since Pulse started.")).color(pal().text_muted));
    } else {
        egui::Frame::new().fill(pal().surface).corner_radius(r_md()).inner_margin(12).show(ui, |ui| {
            ui.set_width(ui.available_width());
            for p in problems.iter().rev() {
                ui.add(egui::Label::new(RichText::new(p).font(egui::FontId::monospace(12.0)).color(pal().text_muted)).wrap());
            }
        });
    }
    let reports = crate::crash::reports().len();
    ui.add_space(10.0);
    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
        let text = match reports {
            0 => tr("No crash reports saved.").to_string(),
            1 => tr("1 crash report saved.").to_string(),
            n => format!("{n} {}", tr("crash reports saved.")),
        };
        ui.label(RichText::new(text).color(pal().text_muted));
        if reports > 0 && ghost_button(ui, icon::FOLDER_OPEN, "Open folder").clicked() {
            ctx.open_url(egui::OpenUrl::new_tab(crate::settings::file_uri(&crate::crash::dir())));
        }
    });
    ui.add_space(30.0);
}

fn heading(ui: &mut Ui, title: &str) {
    ui.add_space(24.0);
    ui.label(RichText::new(title).font(heavy(17.0)).color(pal().text));
    ui.add_space(8.0);
}

/// Width of the names column, so values line up.
const COLUMN: f32 = 190.0;

/// A label in a column `w` wide.
fn fixed(ui: &mut Ui, w: f32, label: egui::Label) {
    ui.allocate_ui_with_layout(egui::vec2(w, 20.0), Layout::left_to_right(Align::Center), |ui| {
        ui.set_min_width(w);
        ui.add(label.truncate());
    });
}

/// "Name    value" on one line.
fn kv(ui: &mut Ui, key: &str, value: &str) {
    ui.horizontal(|ui| {
        fixed(ui, COLUMN, egui::Label::new(RichText::new(key).font(regular(13.5)).color(pal().text_muted)));
        ui.add(egui::Label::new(RichText::new(value).font(med(13.5)).color(pal().text)).wrap());
    });
    ui.add_space(3.0);
}
