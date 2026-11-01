//! The About & privacy page.

use super::{Page, Shell, new_page};
use crate::i18n::{Lang, Msg, tr};
use adw::prelude::*;

const URL: &str = "https://github.com/vipruk/SwiftSwitcher";

fn text_row(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .wrap(true)
        .xalign(0.0)
        .selectable(true)
        .build()
}

pub(super) fn page(_shell: &Shell, lang: Lang) -> adw::PreferencesPage {
    let page = new_page(Page::About, lang);

    let about = adw::PreferencesGroup::builder()
        .title(tr(lang, Msg::AppName))
        .build();
    let version = adw::ActionRow::builder()
        .title(tr(lang, Msg::Version))
        .subtitle(env!("CARGO_PKG_VERSION"))
        .build();
    about.add(&version);
    let site = adw::ActionRow::builder()
        .title(tr(lang, Msg::Website))
        .build();
    site.add_suffix(
        &gtk::LinkButton::builder()
            .uri(URL)
            .label(URL)
            .valign(gtk::Align::Center)
            .build(),
    );
    about.add(&site);
    let license = adw::ActionRow::builder()
        .title(tr(lang, Msg::Licenses))
        .build();
    about.add(&license);
    page.add(&about);

    let privacy = adw::PreferencesGroup::builder()
        .title(tr(lang, Msg::Privacy))
        .build();
    let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
    for m in [Msg::PrivacyReads, Msg::PrivacyLocal, Msg::PrivacyUinput] {
        body.append(&text_row(tr(lang, m)));
    }
    privacy.add(&body);
    page.add(&privacy);
    page
}
