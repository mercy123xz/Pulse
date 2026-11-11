//! Online pages and windows: signing in, public playlists, public profiles.

use super::{Hero, action_row, dialog_button, dialog_frame, hero, playlist_grid, save_heart, tracks_block, with_remote};
use crate::account;
use crate::app::{AccountDialog, AccountDone, AccountMode, App, Page, Route};
use crate::i18n::tr;
use crate::theme::*;
use crate::widgets::*;
use egui::{Align, Color32, CornerRadius, Frame, Key, Layout, Margin, RichText, Stroke, Ui, vec2};
use egui_phosphor::regular as icon;

/// "Mar 2026": when someone joined.
fn month_year(unix: u64) -> String {
    let days = (unix / 86_400) as i64;
    // Civil date from days since 1970 (Howard Hinnant's method).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    const M: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    format!("{} {y}", tr(M[(m - 1) as usize]))
}

/// A labelled text box with an outline, like the import window's.
fn field(ui: &mut Ui, label: &str, text: &mut String, password: bool, id: &str, hint: &str) -> egui::Response {
    ui.label(RichText::new(tr(label)).font(semi(12.5)).color(pal().text_muted));
    ui.add_space(6.0);
    let fid = egui::Id::new(("account_field", id));
    let focused = ui.memory(|m| m.has_focus(fid));
    let r = Frame::new()
        .fill(pal().bg)
        .stroke(Stroke::new(if focused { 1.5 } else { 1.0 }, if focused { pal().accent } else { pal().border }))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(12, 9))
        .show(ui, |ui| {
            ui.add(egui::TextEdit::singleline(text).id(fid).password(password).frame(egui::Frame::NONE).hint_text(tr(hint)).desired_width(f32::INFINITY))
        })
        .inner;
    ui.add_space(14.0);
    r
}

/// The sign-in / create-account window (and change password, delete account, edit bio).
pub fn account_dialog(app: &mut App, ctx: &egui::Context) {
    let Some(d) = &mut app.account_dialog else { return };
    let busy = app.account_busy > 0;
    let mut close = false;
    let mut submit = false;
    let mut app_sign_out = false;
    let mode = d.mode;
    let welcome = d.welcome;
    let required = d.required;
    let modal = egui::Modal::new(egui::Id::new("account")).frame(dialog_frame()).show(ctx, |ui| {
        ui.set_width(420.0);
        ui.spacing_mut().item_spacing.y = 0.0;
        if welcome {
            // The Pulse mark above the welcome.
            let (r, _) = ui.allocate_exact_size(vec2(44.0, 44.0), egui::Sense::hover());
            ui.painter().rect_filled(r, CornerRadius::same(12), pal().accent);
            crate::theme::paint_icon(ui.painter(), r.center(), icon::WAVEFORM, regular(24.0), Color32::WHITE);
            ui.add_space(14.0);
        }
        let (title, body) = match mode {
            AccountMode::SignUp if welcome => {
                ("Welcome to Pulse", "Create a free account to share playlists and have a profile. It's optional: your music and library work without one.")
            }
            AccountMode::SignIn => ("Sign in to Pulse", "Welcome back."),
            AccountMode::SignUp => ("Create a Pulse account", "Share playlists and have a public profile. Your library stays on this computer."),
            AccountMode::Password => ("Change password", "You'll stay signed in here and be signed out everywhere else."),
            AccountMode::Delete => (
                "Delete your account",
                "This deletes your account, your public profile and every playlist you made public. Your library on this computer stays. This can't be undone.",
            ),
            AccountMode::Bio => ("Edit your public profile", "A few words about you. Anyone can see this."),
            AccountMode::Forgot => ("Forgot your password?", "Enter your username or email. We'll email you a 6-digit code to choose a new password."),
            AccountMode::Reset => ("Choose a new password", "Enter the code from the email and a new password. Not in your inbox? Check your spam folder. You'll be signed out on your other computers."),
            AccountMode::Email if required => {
                ("Add your email", "Your account needs an email address, so you can get back in if you forget your password. It's never shown to anyone.")
            }
            AccountMode::Email => ("Change your email", "Password reset codes go to this address. It's never shown to anyone."),
        };
        ui.label(RichText::new(tr(title)).font(heavy(22.0)).color(pal().text));
        ui.add_space(8.0);
        ui.label(RichText::new(tr(body)).font(regular(13.5)).color(pal().text_muted));
        ui.add_space(20.0);
        if let Some(note) = &d.note {
            Frame::new().fill(pal().accent.gamma_multiply(0.16)).corner_radius(CornerRadius::same(8)).inner_margin(Margin::symmetric(12, 9)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(tr(note)).font(regular(12.5)).color(pal().text));
            });
            ui.add_space(14.0);
        }
        if matches!(mode, AccountMode::SignIn | AccountMode::SignUp | AccountMode::Forgot) {
            let (label, hint) = match mode {
                AccountMode::SignUp => ("Username", "3–20 letters, numbers, _ or ."),
                _ => ("Username or email", ""),
            };
            let r = field(ui, label, &mut d.username, false, "user", hint);
            if d.username.is_empty() && !r.has_focus() && !busy {
                r.request_focus();
            }
            if mode == AccountMode::Forgot {
                submit |= r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            }
        }
        let enter = |ui: &Ui, r: &egui::Response| r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        match mode {
            AccountMode::SignIn => {
                let r = field(ui, "Password", &mut d.password, true, "pass", "");
                submit |= enter(ui, &r);
                ui.add_space(-8.0);
                if ui
                    .add(egui::Label::new(RichText::new(tr("Forgot password?")).font(med(12.5)).color(pal().accent_soft)).sense(egui::Sense::click()))
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    d.mode = AccountMode::Forgot;
                    d.error = None;
                    d.password.clear();
                }
                ui.add_space(14.0);
            }
            AccountMode::SignUp => {
                field(ui, "Email", &mut d.email, false, "email", "Only for resetting your password. Never shown.");
                field(ui, "Password", &mut d.password, true, "pass", "At least 8 characters");
                let r = field(ui, "Password again", &mut d.password2, true, "pass2", "");
                submit |= enter(ui, &r);
            }
            AccountMode::Password => {
                field(ui, "Current password", &mut d.password, true, "old", "");
                let r = field(ui, "New password", &mut d.password2, true, "new", "At least 8 characters");
                submit |= enter(ui, &r);
            }
            AccountMode::Delete => {
                let r = field(ui, "Your password", &mut d.password, true, "del", "To make sure it's you");
                submit |= enter(ui, &r);
            }
            AccountMode::Bio => {
                d.bio = d.bio.chars().take(300).collect();
                field(ui, "About you", &mut d.bio, false, "bio", "Up to 300 characters");
            }
            AccountMode::Forgot => {}
            AccountMode::Reset => {
                d.code = d.code.chars().filter(char::is_ascii_digit).take(6).collect();
                let r = field(ui, "Code", &mut d.code, false, "code", "6 digits");
                if d.code.is_empty() && !r.has_focus() && !busy {
                    r.request_focus();
                }
                field(ui, "New password", &mut d.password, true, "new", "At least 8 characters");
                let r = field(ui, "New password again", &mut d.password2, true, "new2", "");
                submit |= enter(ui, &r);
            }
            AccountMode::Email => {
                let r = field(ui, "Email", &mut d.email, false, "email", "you@example.com");
                if d.email.is_empty() && !r.has_focus() && !busy {
                    r.request_focus();
                }
                let r = field(ui, "Your password", &mut d.password, true, "pass", "To make sure it's you");
                submit |= enter(ui, &r);
            }
        }
        if mode == AccountMode::SignUp {
            ui.label(RichText::new(tr("Your username and the playlists you make public can be seen by anyone.")).font(regular(12.0)).color(pal().text_faint));
            ui.add_space(14.0);
        }
        if let Some(e) = &d.error {
            Frame::new().fill(Color32::from_rgb(58, 21, 32)).corner_radius(CornerRadius::same(8)).inner_margin(Margin::symmetric(12, 9)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(tr(e)).font(regular(12.5)).color(Color32::from_rgb(255, 180, 194)));
            });
            ui.add_space(14.0);
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            // Switch between signing in and creating an account.
            let other = match mode {
                AccountMode::SignIn => Some(("New here? Create an account", AccountMode::SignUp)),
                AccountMode::SignUp => Some(("Have an account? Sign in", AccountMode::SignIn)),
                AccountMode::Forgot => Some(("Back to sign in", AccountMode::SignIn)),
                AccountMode::Reset => Some(("Send a new code", AccountMode::Forgot)),
                _ => None,
            };
            if let Some((label, to)) = other
                && ui
                    .add(egui::Label::new(RichText::new(tr(label)).font(med(12.5)).color(pal().accent_soft)).sense(egui::Sense::click()))
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
            {
                d.mode = to;
                d.error = None;
                d.note = None;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                let label = match mode {
                    AccountMode::SignIn => "Sign in",
                    AccountMode::SignUp => "Create account",
                    AccountMode::Password => "Change password",
                    AccountMode::Delete => "Delete account",
                    AccountMode::Bio | AccountMode::Email => "Save",
                    AccountMode::Forgot => "Send code",
                    AccountMode::Reset => "Set password",
                };
                if busy {
                    ui.spinner();
                } else if dialog_button(ui, tr(label), true, true).clicked() {
                    submit = true;
                }
                let cancel = if required {
                    "Sign out"
                } else if welcome {
                    "Not now"
                } else {
                    "Cancel"
                };
                if dialog_button(ui, tr(cancel), false, !busy).clicked() {
                    close = true;
                    if required {
                        app_sign_out = true;
                    }
                }
            });
        });
    });
    if submit && !busy {
        submit_dialog(app, ctx);
    }
    if app_sign_out {
        app.account_dialog = None;
        app.account_job(ctx, || {
            account::sign_out();
            Ok(AccountDone::SignedOut)
        });
        return;
    }
    // An account without an email can't wave the window away (only "Sign out" closes it).
    if (close || (modal.should_close() && !required)) && app.account_busy == 0 {
        app.account_dialog = None;
        // "Not now" on the welcome: the profile window (name, picture) comes next, as before.
        if welcome && app.saved.profile.created == 0 && !app.saved.profile.skipped {
            app.profile_editing = Some(crate::ui::ProfileForm { welcome: true, ..Default::default() });
        }
    }
}

fn submit_dialog(app: &mut App, ctx: &egui::Context) {
    let Some(d) = &mut app.account_dialog else { return };
    let (user, pass, pass2, bio) = (d.username.trim().to_lowercase(), d.password.clone(), d.password2.clone(), d.bio.trim().to_string());
    let (email, code) = (d.email.trim().to_lowercase(), d.code.clone());
    d.note = None;
    // Checks the server would make anyway, said straight away.
    let local_error = match d.mode {
        AccountMode::SignIn if user.is_empty() || pass.is_empty() => Some("Enter your username or email, and your password."),
        AccountMode::SignUp if user.len() < 3 => Some("Usernames are 3 to 20 characters."),
        AccountMode::SignUp if !account::valid_username(&user) => Some("Usernames can use letters, numbers, _ and . only."),
        AccountMode::SignUp | AccountMode::Email if email.is_empty() => Some("Enter your email address."),
        AccountMode::SignUp | AccountMode::Email if !account::valid_email(&email) => Some("That email address doesn't look right."),
        AccountMode::Email if pass.is_empty() => Some("Enter your password."),
        AccountMode::Forgot if user.is_empty() => Some("Enter your username or email address."),
        AccountMode::Reset if code.len() != 6 => Some("Enter the 6-digit code from the email."),
        AccountMode::Reset if pass.chars().count() < 8 => Some("Passwords need at least 8 characters."),
        AccountMode::Reset if pass != pass2 => Some("The two passwords don't match."),
        AccountMode::SignUp | AccountMode::Password if (if d.mode == AccountMode::SignUp { &pass } else { &pass2 }).chars().count() < 8 => {
            Some("Passwords need at least 8 characters.")
        }
        AccountMode::SignUp if pass != pass2 => Some("The two passwords don't match."),
        AccountMode::Delete if pass.is_empty() => Some("Enter your password."),
        _ => None,
    };
    if let Some(e) = local_error {
        d.error = Some(e.to_string());
        return;
    }
    let reload = account::session().map(|s| format!("user:{}", s.username));
    match d.mode {
        AccountMode::SignIn => app.account_job(ctx, move || account::sign_in(&user, &pass).map(AccountDone::SignedIn)),
        AccountMode::SignUp => app.account_job(ctx, move || account::sign_up(&user, &email, &pass).map(AccountDone::SignedIn)),
        AccountMode::Forgot => app.account_job(ctx, move || account::reset_start(&user).map(AccountDone::CodeSent)),
        AccountMode::Reset => app.account_job(ctx, move || account::reset_finish(&user, &code, &pass).map(AccountDone::SignedIn)),
        AccountMode::Email => {
            app.account_job(ctx, move || account::set_email(&email, &pass).map(|_| AccountDone::Done { message: tr("Email saved").to_string(), reload: None }))
        }
        AccountMode::Password => app.account_job(ctx, move || {
            account::change_password(&pass, &pass2).map(|_| AccountDone::Done { message: tr("Password changed").to_string(), reload: None })
        }),
        AccountMode::Delete => app.account_job(ctx, move || account::delete_account(&pass).map(|_| AccountDone::Deleted)),
        AccountMode::Bio => {
            app.account_job(ctx, move || account::set_bio(&bio).map(|_| AccountDone::Done { message: tr("Profile saved").to_string(), reload }))
        }
    }
}

/// Under the header of your own playlist: make it public, or its link and "Make private".
pub fn publish_row(app: &mut App, ui: &mut Ui, i: usize) {
    if !account::available() {
        return;
    }
    let ctx = ui.ctx().clone();
    let Some(p) = app.saved.playlists.get(i) else { return };
    let public = p.public_id.clone();
    let shareable = account::shareable(&p.tracks).len();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        match &public {
            None => {
                let ok = shareable > 0 && app.account_busy == 0;
                if ui.add_enabled_ui(ok, |ui| ghost_button(ui, icon::GLOBE_HEMISPHERE_WEST, "Make public")).inner.clicked() {
                    app.publish_playlist(i, &ctx);
                }
                if shareable == 0 && !app.saved.playlists[i].tracks.is_empty() {
                    ui.label(RichText::new(tr("Songs from your computer can't be shared.")).font(regular(12.0)).color(pal().text_faint));
                }
            }
            Some(id) => {
                Frame::new().fill(pal().accent.gamma_multiply(0.18)).corner_radius(CornerRadius::same(12)).inner_margin(Margin::symmetric(10, 4)).show(
                    ui,
                    |ui| {
                        ui.label(RichText::new(format!("{}  {}", icon::GLOBE_HEMISPHERE_WEST, tr("Public"))).font(semi(12.0)).color(pal().accent_soft));
                    },
                );
                if ghost_button(ui, icon::LINK, "Copy link").clicked() {
                    ctx.copy_text(account::playlist_link(id));
                    app.toast(tr("Link copied"), &ctx);
                }
                if ghost_button(ui, icon::EYE, "View").clicked() {
                    app.navigate(Route::Public(id.clone()), &ctx);
                }
                if ghost_button(ui, icon::LOCK_SIMPLE, "Make private").clicked() {
                    app.unpublish_playlist(i, &ctx);
                }
                let local = app.saved.playlists[i].tracks.len() - shareable;
                if local > 0 {
                    ui.label(RichText::new(format!("{local} {}", tr("songs from your computer aren't shared"))).font(regular(12.0)).color(pal().text_faint));
                }
            }
        }
    });
    ui.add_space(6.0);
}

/// A playlist someone made public.
pub fn public_page(app: &mut App, ui: &mut Ui) {
    with_remote(app, ui, |app, ui, page| {
        let Page::Public(p) = page else { return };
        let ctx = ui.ctx().clone();
        // The header shows the owner's name (a link) before this.
        let sub = format!(" · {} {}", p.tracks.len(), tr(if p.tracks.len() == 1 { "song" } else { "songs" }));
        let clicks = hero(
            ui,
            &Hero {
                editable: false,
                kind: tr("Public playlist"),
                title: &p.name,
                subtitle: &sub,
                art: p.cover.as_deref(),
                glyph: icon::PLAYLIST,
                round: false,
                banner: None,
                links: &[p.username.as_str()],
            },
        );
        if clicks.link.is_some() {
            app.navigate(Route::User(p.username.clone()), &ctx);
        }
        if !p.description.is_empty() {
            ui.label(RichText::new(&p.description).font(regular(13.5)).color(pal().text_muted));
            ui.add_space(8.0);
        }
        let mine = account::session().is_some_and(|s| s.username == p.username);
        ui.horizontal(|ui| {
            action_row(app, ui, &p.tracks);
            let info = p.info();
            let liked = app.saved.liked_playlists.iter().any(|x| x.id == info.id);
            if !mine && save_heart(ui, liked).clicked() {
                app.toggle_playlist(&info, &ctx);
            }
            if ghost_button(ui, icon::LINK, "Copy link").clicked() {
                ctx.copy_text(account::playlist_link(&p.id));
                app.toast(tr("Link copied"), &ctx);
            }
            if !mine && ghost_button(ui, icon::COPY, "Save a copy").clicked() {
                app.save_copy(p, &ctx);
            }
            if !mine {
                let id = p.id.clone();
                let r = icon_button(ui, icon::DOTS_THREE, 18.0, pal().text_muted, tr("More"));
                egui::Popup::menu(&r).show(|ui| {
                    menu_begin(ui);
                    if menu_item(ui, icon::FLAG, "Report playlist") {
                        app.account_job(&ctx, move || {
                            account::report(&id, "Reported from Pulse")
                                .map(|_| AccountDone::Done { message: tr("Thanks. We'll take a look.").to_string(), reload: None })
                        });
                    }
                });
            }
        });
        tracks_block(app, ui, "public", &p.tracks);
    });
}

/// Someone's public profile.
pub fn user_page(app: &mut App, ui: &mut Ui) {
    with_remote(app, ui, |app, ui, page| {
        let Page::User(p) = page else { return };
        let ctx = ui.ctx().clone();
        let n = p.playlists.len();
        let mut sub = format!("{} {} · {} {}", n, tr(if n == 1 { "public playlist" } else { "public playlists" }), tr("Joined"), month_year(p.user.created));
        if !p.user.bio.is_empty() {
            sub = format!("{} · {sub}", p.user.bio);
        }
        let title = format!("@{}", p.user.username);
        hero(
            ui,
            &Hero { editable: false, kind: tr("Profile"), title: &title, subtitle: &sub, art: None, glyph: icon::USER, round: true, banner: None, links: &[] },
        );
        let mine = account::session().is_some_and(|s| s.username == p.user.username);
        ui.horizontal(|ui| {
            if ghost_button(ui, icon::LINK, "Copy link").clicked() {
                ctx.copy_text(account::profile_link(&p.user.username));
                app.toast(tr("Link copied"), &ctx);
            }
            // Your own profile on the website; it then greets you by name on Pulse's home page.
            if mine && ghost_button(ui, icon::GLOBE, "Open on the website").clicked() {
                ctx.open_url(egui::OpenUrl::new_tab(format!("{}&me=1", account::profile_link(&p.user.username))));
            }
            if mine && ghost_button(ui, icon::PENCIL_SIMPLE, "Edit profile").clicked() {
                let mut d = AccountDialog::new(AccountMode::Bio);
                d.bio = p.user.bio.clone();
                app.account_dialog = Some(d);
            }
        });
        ui.add_space(8.0);
        if p.playlists.is_empty() {
            let body = if mine { "Open one of your playlists and press “Make public”." } else { "Nothing public yet." };
            empty_state(ui, icon::PLAYLIST, "No public playlists", body, None);
        } else {
            section_title(ui, "Public playlists", None);
            let cards: Vec<_> = p.playlists.iter().map(|l| l.info(&p.user.username)).collect();
            playlist_grid(app, ui, &cards, None);
        }
    });
}

/// A Settings row: (ui, title, description, the control on the right).
pub type Row<'a> = &'a dyn Fn(&mut Ui, &str, &str, &mut dyn FnMut(&mut Ui));

/// The Account part of Settings.
pub fn account_settings(app: &mut App, ui: &mut Ui, row: Row) {
    if !account::available() {
        return;
    }
    let ctx = ui.ctx().clone();
    match account::session() {
        None => {
            row(
                ui,
                tr("Pulse account"),
                tr("Make playlists public and have a profile people can find. Optional: everything else works without one."),
                &mut |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if ui.button(tr("Create account")).clicked() {
                        app.account_dialog = Some(AccountDialog::new(AccountMode::SignUp));
                    }
                    if ui.button(tr("Sign in")).clicked() {
                        app.account_dialog = Some(AccountDialog::new(AccountMode::SignIn));
                    }
                },
            );
        }
        Some(s) => {
            let who = format!("{} @{}", tr("Signed in as"), s.username);
            row(ui, &who, tr("Your library stays on this computer. Only playlists you make public are online."), &mut |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if ui.button(tr("Sign out")).clicked() {
                    app.account_job(&ctx, || {
                        account::sign_out();
                        Ok(AccountDone::SignedOut)
                    });
                }
                if ui.button(tr("Your profile")).clicked() {
                    app.navigate(Route::User(s.username.clone()), &ctx);
                }
            });
            let email = if s.email.is_empty() { tr("None yet. Add one so you can reset your password.").to_string() } else { s.email.clone() };
            row(ui, tr("Email"), &email, &mut |ui| {
                if ui.button(tr(if s.email.is_empty() { "Add email" } else { "Change email" })).clicked() {
                    let mut d = AccountDialog::new(AccountMode::Email);
                    d.email = s.email.clone();
                    d.required = false;
                    app.account_dialog = Some(d);
                }
            });
            row(ui, tr("Password"), tr("Change the password you sign in with."), &mut |ui| {
                if ui.button(tr("Change password")).clicked() {
                    app.account_dialog = Some(AccountDialog::new(AccountMode::Password));
                }
            });
            row(ui, tr("Delete account"), tr("Deletes your account and everything you made public. Your library on this computer stays."), &mut |ui| {
                if ui.button(RichText::new(tr("Delete account")).color(Color32::from_rgb(255, 140, 160))).clicked() {
                    app.account_dialog = Some(AccountDialog::new(AccountMode::Delete));
                }
            });
        }
    }
}
