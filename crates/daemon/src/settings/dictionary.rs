//! The Dictionary page: the never-convert and always-convert word lists.

use super::list::ItemList;
use super::{Page, Shell, blocking, new_page};
use crate::config::data_dir;
use crate::i18n::{Lang, Msg, tr};
use adw::prelude::*;
use gtk::glib;
use std::rc::Rc;
use swift_engine::dict::{List, UserDict};

fn items(dict: &UserDict, which: List) -> Vec<String> {
    let set = match which {
        List::Never => &dict.never,
        List::Always => &dict.always,
    };
    let mut v: Vec<String> = set.iter().cloned().collect();
    v.sort();
    v
}

/// Re-reads the file and shows it.
fn reload(shell: &Shell, lang: Lang, list: &Rc<ItemList>, which: List) {
    let (list, shell) = (list.clone(), shell.clone());
    glib::spawn_future_local(async move {
        match blocking(|| UserDict::load(&data_dir())).await {
            Some(Ok(d)) => list.show(&items(&d, which)),
            Some(Err(e)) => {
                log::warn!("cannot read the dictionary: {e}");
                shell.toast(tr(lang, Msg::SaveFailed));
            }
            None => {}
        }
    });
}

fn edit(shell: &Shell, lang: Lang, list: &Rc<ItemList>, which: List, add: &str, remove: &str) {
    let (list, shell) = (list.clone(), shell.clone());
    let (add, remove) = (add.to_string(), remove.to_string());
    glib::spawn_future_local(async move {
        let adds = if add.is_empty() { vec![] } else { vec![add] };
        let removes = if remove.is_empty() {
            vec![]
        } else {
            vec![remove]
        };
        let saved =
            blocking(move || UserDict::save_edit(&data_dir(), which, &adds, &removes)).await;
        if !matches!(saved, Some(Ok(()))) {
            if let Some(Err(e)) = saved {
                log::warn!("cannot save the dictionary: {e}");
            }
            shell.toast(tr(lang, Msg::SaveFailed));
        }
        reload(&shell, lang, &list, which);
    });
}

fn group(shell: &Shell, lang: Lang, which: List, title: Msg, hint: Msg) -> Rc<ItemList> {
    let (s1, s2) = (shell.clone(), shell.clone());
    let list = ItemList::new(
        lang,
        title,
        hint,
        Msg::WordPlaceholder,
        move |l, w| edit(&s1, lang, l, which, &w, ""),
        move |l, w| edit(&s2, lang, l, which, "", &w),
    );
    reload(shell, lang, &list, which);
    // The daemon appends learned words; pick them up on return to the window.
    let (weak, s) = (Rc::downgrade(&list), shell.clone());
    shell.window.connect_is_active_notify(move |w| {
        if let (true, Some(list)) = (w.is_active(), weak.upgrade()) {
            reload(&s, lang, &list, which);
        }
    });
    list
}

pub(super) fn page(shell: &Shell, lang: Lang) -> adw::PreferencesPage {
    let page = new_page(Page::Dictionary, lang);
    let never = group(
        shell,
        lang,
        List::Never,
        Msg::NeverConvert,
        Msg::NeverConvertHint,
    );
    let always = group(
        shell,
        lang,
        List::Always,
        Msg::AlwaysConvert,
        Msg::AlwaysConvertHint,
    );
    page.add(&never.group);
    page.add(&always.group);
    page
}
