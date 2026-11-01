//! A group with an entry that adds items and one row per item that removes
//! it. Used by the Dictionary and Applications pages.

use crate::i18n::{Lang, Msg, tr};
use adw::prelude::*;
use std::cell::RefCell;
use std::rc::{Rc, Weak};

type Callback = Rc<dyn Fn(&Rc<ItemList>, String)>;

pub(super) struct ItemList {
    pub group: adw::PreferencesGroup,
    rows: RefCell<Vec<adw::ActionRow>>,
    lang: Lang,
    on_remove: Callback,
}

impl ItemList {
    pub fn new(
        lang: Lang,
        title: Msg,
        description: Msg,
        placeholder: Msg,
        on_add: impl Fn(&Rc<ItemList>, String) + 'static,
        on_remove: impl Fn(&Rc<ItemList>, String) + 'static,
    ) -> Rc<ItemList> {
        let group = adw::PreferencesGroup::builder()
            .title(tr(lang, title))
            .description(tr(lang, description))
            .build();
        let entry = adw::EntryRow::builder()
            .title(tr(lang, placeholder))
            .show_apply_button(true)
            .build();
        group.add(&entry);
        let list = Rc::new(ItemList {
            group,
            rows: RefCell::default(),
            lang,
            on_remove: Rc::new(on_remove),
        });
        let weak: Weak<ItemList> = Rc::downgrade(&list);
        entry.connect_apply(move |e| {
            let text = e.text().trim().to_string();
            e.set_text("");
            if let (false, Some(list)) = (text.is_empty(), weak.upgrade()) {
                on_add(&list, text);
            }
        });
        list
    }

    /// Replaces the item rows.
    pub fn show(self: &Rc<Self>, items: &[String]) {
        for row in self.rows.borrow_mut().drain(..) {
            self.group.remove(&row);
        }
        let mut rows = Vec::new();
        for item in items {
            let row = adw::ActionRow::builder()
                .title(glib_escape(item))
                .use_markup(true)
                .build();
            let remove = gtk::Button::builder()
                .icon_name("user-trash-symbolic")
                .tooltip_text(tr(self.lang, Msg::Remove))
                .valign(gtk::Align::Center)
                .css_classes(["flat"])
                .build();
            let (weak, item) = (Rc::downgrade(self), item.clone());
            remove.connect_clicked(move |_| {
                if let Some(list) = weak.upgrade() {
                    (list.on_remove.clone())(&list, item.clone());
                }
            });
            row.add_suffix(&remove);
            self.group.add(&row);
            rows.push(row);
        }
        *self.rows.borrow_mut() = rows;
    }
}

fn glib_escape(s: &str) -> String {
    gtk::glib::markup_escape_text(s).to_string()
}
