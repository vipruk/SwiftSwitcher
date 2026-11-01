//! The Applications page: apps where auto-correction stays off.

use super::list::ItemList;
use super::{Page, Shell, new_page, off_thread, update_config};
use crate::config::{Config, config_path};
use crate::desktop::{Desktop, GnomeDesktop};
use crate::i18n::{Lang, Msg, tr};
use adw::prelude::*;
use gtk::glib;
use std::rc::Rc;

fn excluded() -> Vec<String> {
    Config::load_or_default(&config_path())
        .unwrap_or_else(|e| {
            log::warn!("{e:#}");
            Config::default()
        })
        .excluded_apps
}

fn change(shell: &Shell, lang: Lang, list: &Rc<ItemList>, f: impl FnOnce(&mut Vec<String>)) {
    if let Err(e) = update_config(|c| f(&mut c.excluded_apps)) {
        log::warn!("cannot save the exceptions: {e:#}");
        shell.toast(tr(lang, Msg::SaveFailed));
    }
    list.show(&excluded());
}

fn add_app(shell: &Shell, lang: Lang, list: &Rc<ItemList>, id: &str) {
    let id = id.trim();
    if id.is_empty() {
        return;
    }
    change(shell, lang, list, |v| {
        if !v.iter().any(|a| a.eq_ignore_ascii_case(id)) {
            v.push(id.to_string());
        }
    });
}

pub(super) fn page(shell: &Shell, lang: Lang) -> adw::PreferencesPage {
    let page = new_page(Page::Apps, lang);
    let (s1, s2) = (shell.clone(), shell.clone());
    let list = ItemList::new(
        lang,
        Msg::ExcludedApps,
        Msg::ExcludedAppsHint,
        Msg::AppPlaceholder,
        move |l, id| add_app(&s1, lang, l, &id),
        move |l, id| change(&s2, lang, l, |v| v.retain(|a| a != &id)),
    );
    list.show(&excluded());
    page.add(&list.group);

    let group = adw::PreferencesGroup::new();
    let button = gtk::Button::builder()
        .label(tr(lang, Msg::AddRunningApp))
        .halign(gtk::Align::Start)
        .build();
    let (weak, s) = (Rc::downgrade(&list), shell.clone());
    button.connect_clicked(move |b| {
        let (weak, s, b) = (weak.clone(), s.clone(), b.clone());
        b.set_sensitive(false);
        glib::spawn_future_local(async move {
            let apps =
                off_thread(async { GnomeDesktop::connect().await?.running_apps().await }).await;
            b.set_sensitive(true);
            let apps = match apps {
                Some(Ok(a)) => Some(a),
                Some(Err(e)) => {
                    log::info!("cannot list running apps: {e:#}");
                    None
                }
                None => None,
            };
            if let Some(list) = weak.upgrade() {
                pick_app(
                    &s,
                    lang,
                    &list,
                    apps,
                    b.root().and_then(|r| r.dynamic_cast::<gtk::Widget>().ok()),
                );
            }
        });
    });
    group.add(&button);
    page.add(&group);
    page
}

/// Running apps not yet excluded, sorted, without duplicates.
fn candidates(running: Vec<String>, excluded: &[String]) -> Vec<String> {
    let mut v: Vec<String> = running
        .into_iter()
        .filter(|a| !a.is_empty() && !excluded.iter().any(|e| e.eq_ignore_ascii_case(a)))
        .collect();
    v.sort_by_key(|a| a.to_lowercase());
    v.dedup();
    v
}

/// `apps` is None when the extension is unreachable; the user can still
/// type an id.
fn pick_app(
    shell: &Shell,
    lang: Lang,
    list: &Rc<ItemList>,
    apps: Option<Vec<String>>,
    parent: Option<gtk::Widget>,
) {
    let dialog = adw::AlertDialog::new(Some(tr(lang, Msg::AddRunningApp)), None);
    dialog.add_response("cancel", tr(lang, Msg::Cancel));
    dialog.add_response("add", tr(lang, Msg::AddButton));
    dialog.set_response_appearance("add", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("add"));
    dialog.set_close_response("cancel");

    let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
    let entry = gtk::Entry::builder()
        .placeholder_text(tr(lang, Msg::TypeAppId))
        .activates_default(true)
        .build();
    match apps {
        None => dialog.set_body(tr(lang, Msg::ProblemExtension)),
        Some(running) => {
            dialog.set_body(tr(lang, Msg::RunningApps));
            let rows = gtk::ListBox::builder()
                .css_classes(["boxed-list"])
                .selection_mode(gtk::SelectionMode::Single)
                .build();
            for id in candidates(running, &excluded()) {
                rows.append(
                    &adw::ActionRow::builder()
                        .title(glib::markup_escape_text(&id))
                        .build(),
                );
                // Row titles are escaped; keep the raw id on the row.
                if let Some(row) = rows.last_child() {
                    row.set_widget_name(&id);
                }
            }
            let e = entry.clone();
            rows.connect_row_selected(move |_, row| {
                if let Some(row) = row {
                    e.set_text(&row.widget_name());
                }
            });
            let scroll = gtk::ScrolledWindow::builder()
                .child(&rows)
                .min_content_height(180)
                .max_content_height(280)
                .propagate_natural_height(true)
                .build();
            body.append(&scroll);
        }
    }
    body.append(&entry);
    dialog.set_extra_child(Some(&body));

    let (list, shell) = (list.clone(), shell.clone());
    dialog.connect_response(None, move |_, response| {
        if response == "add" {
            add_app(&shell, lang, &list, &entry.text());
        }
    });
    dialog.present(parent.as_ref());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidates_skip_excluded_and_duplicates() {
        let running = ["b.App", "a.app", "", "b.App", "kitty"]
            .map(String::from)
            .to_vec();
        let excluded = vec!["KITTY".to_string()];
        assert_eq!(candidates(running, &excluded), ["a.app", "b.App"]);
    }
}
