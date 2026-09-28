// SPDX-License-Identifier: GPL-3.0-only

//! The Pocket application shell.
//!
//! Deliberately thin. It lists what the store holds, filters by style, draws
//! the selected pass, and puts its barcode full-screen when asked. Everything
//! about *what a pass is* lives in `pocket-core`; how a pass looks is in
//! [`crate::face`] and [`crate::presenter`].

use std::path::PathBuf;

use cosmic::app::{Core, Task, context_drawer};
use cosmic::iced::Length;
use cosmic::prelude::*;
use cosmic::widget::about::About;
use cosmic::widget::{self, nav_bar};
use pocket_core::{Listing, Pass, PassKind, PassStore, StoredPass, Symbol, UnreadablePass};

use crate::fl;
use crate::screen::{self, Hold};

const APP_ID: &str = "com.magnetaros.Pocket";
const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");
const APP_ICON: &[u8] =
    include_bytes!("../resources/icons/hicolor/scalable/apps/com.magnetaros.Pocket.svg");

/// What a sidebar row filters the list to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Filter {
    All,
    Kind(PassKind),
}

#[derive(Clone, Debug)]
pub enum Message {
    Select(usize),
    /// Put the selected pass's barcode on the whole screen.
    Present,
    /// Come back from the presenter.
    Leave,
    /// The screen state the presenter borrowed, once the portal and the
    /// settings daemon have answered.
    ScreenHeld(Hold),
    LaunchUrl(String),
    ToggleAbout,
}

pub struct AppModel {
    core: Core,
    about: About,
    nav: nav_bar::Model,
    /// Everything the store holds, in its own order. The sidebar filters this
    /// rather than reloading, so switching rows costs no disk read.
    passes: Vec<StoredPass>,
    /// Passes on disk that would not parse. Surfaced rather than dropped — a
    /// wallet that silently shows fewer passes than the directory holds is
    /// the failure a traveller finds at the gate.
    unreadable: Vec<UnreadablePass>,
    /// How many of `passes`, from the front, were opened from files named on
    /// the command line rather than read from the store. They are shown, not
    /// kept: import needs the crash-safe writer (ROADMAP milestone 3).
    opened: usize,
    /// Index into `passes`, not into the filtered view: the filter changes,
    /// the selection should not follow it to a different pass.
    selected: Option<usize>,
    /// The selected pass's barcode, encoded once when it is selected rather
    /// than on every frame — a PDF417 symbol is not free to compute, and it
    /// cannot change while the pass sits there. `None` when the pass carries
    /// no barcode at all, which is ordinary for a coupon or a generic pass.
    symbol: Option<Result<Symbol, String>>,
    /// Whether the barcode has the whole screen.
    presenting: bool,
    /// What the presenter borrowed from the desktop and owes back.
    hold: Hold,
    root: String,
    /// Set when the store itself could not be opened, which is a different
    /// condition from an empty wallet and reads differently to the user.
    fatal: Option<String>,
}

impl AppModel {
    fn filter(&self) -> Filter {
        self.nav
            .active_data::<Filter>()
            .copied()
            .unwrap_or(Filter::All)
    }

    /// The passes the current sidebar row shows, with their index in
    /// `self.passes` so a click can select the right one.
    fn visible(&self) -> Vec<(usize, &StoredPass)> {
        let filter = self.filter();
        self.passes
            .iter()
            .enumerate()
            .filter(|(_, stored)| match filter {
                Filter::All => true,
                Filter::Kind(kind) => stored.pass.kind == kind,
            })
            .collect()
    }

    fn selected_pass(&self) -> Option<&Pass> {
        self.selected
            .and_then(|index| self.passes.get(index))
            .map(|stored| &stored.pass)
    }

    /// The symbol the presenter would show, if there is one.
    fn presentable(&self) -> Option<(&Pass, &Symbol)> {
        let pass = self.selected_pass()?;
        match self.symbol.as_ref()? {
            Ok(symbol) => Some((pass, symbol)),
            Err(_) => None,
        }
    }

    fn list(&self) -> Element<'_, Message> {
        let visible = self.visible();
        let now = now();
        if visible.is_empty() {
            return widget::text::body(fl!("no-passes")).into();
        }

        let mut column = widget::list_column();
        for (index, stored) in visible {
            let pass = &stored.pass;
            let mut lines = widget::column::with_capacity(2)
                .push(widget::text::body(pass.title().to_owned()))
                .spacing(2);
            if let Some(detail) = summary(pass, now) {
                lines = lines.push(widget::text::caption(detail));
            }
            column = column.add(
                widget::button::custom(lines)
                    .on_press(Message::Select(index))
                    .width(Length::Fill)
                    .class(cosmic::theme::Button::Text),
            );
        }
        widget::scrollable(column).height(Length::Fill).into()
    }

    /// One line per pass that would not read: which one, and why. A count
    /// alone does not say *which* boarding pass is broken.
    fn unreadable_lines(&self) -> Vec<String> {
        self.unreadable
            .iter()
            .map(|failure| {
                fl!(
                    "unreadable-pass",
                    id = failure.id.clone(),
                    reason = failure.reason.clone()
                )
            })
            .collect()
    }

    fn detail(&self) -> Element<'_, Message> {
        let Some(pass) = self.selected_pass() else {
            return widget::text::body(fl!("select-a-pass")).into();
        };
        let face = crate::face::view(pass, self.symbol.as_ref(), now());
        if self.selected.is_some_and(|index| index < self.opened) {
            return widget::column::with_capacity(2)
                .spacing(cosmic::theme::spacing().space_xs)
                .push(widget::text::caption(fl!("opened-from-file")))
                .push(face)
                .into();
        }
        face
    }

    /// Selects a pass and encodes its barcode, once.
    fn select(&mut self, index: usize) {
        self.selected = Some(index);
        self.symbol = self
            .passes
            .get(index)
            .and_then(|stored| stored.pass.barcode())
            .map(|barcode| pocket_core::barcode::encode(barcode).map_err(|why| why.to_string()));
    }

    /// Enters or leaves the presenter, moving the window and the desktop
    /// state with it.
    fn present(&mut self, presenting: bool) -> Task<Message> {
        self.presenting = presenting;
        // The header bar and the sidebar are chrome, and chrome is screen a
        // reader could have had.
        self.core.window.show_headerbar = !presenting;
        self.core.nav_bar_set_toggled(!presenting);

        let mode = if presenting {
            cosmic::iced::window::Mode::Fullscreen
        } else {
            cosmic::iced::window::Mode::Windowed
        };
        let window = self
            .core
            .main_window_id()
            .map_or_else(Task::none, |id| cosmic::iced::window::set_mode(id, mode));

        let desktop = if presenting {
            let reason = fl!("presenting-a-pass");
            cosmic::task::future(async move { Message::ScreenHeld(screen::acquire(reason).await) })
        } else {
            let hold = std::mem::take(&mut self.hold);
            cosmic::task::future(async move {
                screen::release(hold).await;
                Message::ScreenHeld(Hold::default())
            })
        };

        Task::batch([window, desktop])
    }
}

/// The most of a file read before handing it to the reader, which refuses an
/// archive over 64 MiB: one byte more than that is enough for it to say so,
/// and a file of any size is never loaded whole just to be refused.
const OPEN_FILE_LIMIT: u64 = 64 * 1024 * 1024 + 1;

/// Reads the `.pkpass` files named on the command line, for showing.
///
/// Each becomes a pass, or an unreadable entry naming the file and saying
/// why: a file the user explicitly opened must not vanish without a word.
fn open_files(files: &[PathBuf]) -> (Vec<StoredPass>, Vec<UnreadablePass>) {
    use std::io::Read as _;

    let mut passes = Vec::new();
    let mut unreadable = Vec::new();
    for path in files {
        let mut bytes = Vec::new();
        let read = std::fs::File::open(path)
            .and_then(|file| file.take(OPEN_FILE_LIMIT).read_to_end(&mut bytes))
            .map_err(|why| why.to_string())
            .and_then(|_| pocket_core::pkpass::read(&bytes).map_err(|why| why.to_string()));
        let id = path.file_name().map_or_else(
            || path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        match read {
            Ok(pass) => passes.push(StoredPass {
                id,
                path: path.clone(),
                pass,
            }),
            Err(reason) => unreadable.push(UnreadablePass { id, reason }),
        }
    }
    (passes, unreadable)
}

/// The time to judge a pass's expiry by.
fn now() -> chrono::DateTime<chrono::FixedOffset> {
    chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now()).fixed_offset()
}

/// The one line under a pass's name in the list.
///
/// The primary fields joined, which for a boarding pass reads `ATH → LHR`
/// and for a store card is usually the single balance or member number,
/// followed by "Expired" or "Voided" when the pass is no longer good.
fn summary(pass: &Pass, now: chrono::DateTime<chrono::FixedOffset>) -> Option<String> {
    let values: Vec<&str> = pass
        .primary_fields
        .iter()
        .map(|field| field.value.as_str())
        .filter(|value| !value.is_empty())
        .collect();
    let status = crate::face::status(pass, now);
    match (values.is_empty(), status) {
        (true, None) => None,
        (true, Some(status)) => Some(status),
        (false, None) => Some(values.join(" → ")),
        (false, Some(status)) => Some(format!("{} · {status}", values.join(" → "))),
    }
}

fn label(filter: Filter) -> String {
    match filter {
        Filter::All => fl!("all-passes"),
        Filter::Kind(PassKind::BoardingPass) => fl!("boarding-passes"),
        Filter::Kind(PassKind::EventTicket) => fl!("event-tickets"),
        Filter::Kind(PassKind::StoreCard) => fl!("store-cards"),
        Filter::Kind(PassKind::Coupon) => fl!("coupons"),
        Filter::Kind(PassKind::Generic) => fl!("generic-passes"),
    }
}

impl cosmic::Application for AppModel {
    type Executor = cosmic::executor::Default;
    /// Files to show, from "Open with Pocket".
    type Flags = Vec<PathBuf>;
    type Message = Message;
    const APP_ID: &'static str = APP_ID;

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, files: Self::Flags) -> (Self, Task<Self::Message>) {
        let about = About::default()
            .name(fl!("app-title"))
            .icon(widget::icon::from_svg_bytes(APP_ICON))
            .version(env!("CARGO_PKG_VERSION"))
            .license(env!("CARGO_PKG_LICENSE"))
            .links([(fl!("repository"), REPOSITORY)]);

        let (store, fatal) = match PassStore::open_default() {
            Ok(store) => (Some(store), None),
            Err(why) => {
                tracing::error!(%why, "cannot open the pass store");
                (None, Some(why.to_string()))
            }
        };

        let root = store
            .as_ref()
            .map(|store| store.root().display().to_string())
            .unwrap_or_default();

        let Listing {
            passes: stored,
            unreadable: stored_unreadable,
        } = store
            .as_ref()
            .map(|store| match store.list() {
                Ok(listing) => listing,
                Err(why) => {
                    tracing::error!(%why, "cannot list passes");
                    Listing::default()
                }
            })
            .unwrap_or_default();

        let mut nav = nav_bar::Model::default();
        nav.insert()
            .text(label(Filter::All))
            .data(Filter::All)
            .activate();
        for kind in PassKind::all() {
            let filter = Filter::Kind(kind);
            nav.insert().text(label(filter)).data(filter);
        }

        // Files handed over by the file manager come first and the first of
        // them is selected: it is what the user just asked to see.
        let (mut passes, mut unreadable) = open_files(&files);
        let opened = passes.len();
        passes.extend(stored);
        unreadable.extend(stored_unreadable);

        let mut app = Self {
            core,
            about,
            nav,
            passes,
            unreadable,
            opened,
            selected: None,
            symbol: None,
            presenting: false,
            hold: Hold::default(),
            root,
            fatal,
        };
        if opened > 0 {
            app.select(0);
        }
        (app, Task::none())
    }

    fn nav_model(&self) -> Option<&nav_bar::Model> {
        Some(&self.nav)
    }

    fn on_nav_select(&mut self, id: nav_bar::Id) -> Task<Self::Message> {
        self.nav.activate(id);
        Task::none()
    }

    /// Escape leaves the presenter, and only the presenter.
    fn on_escape(&mut self) -> Task<Self::Message> {
        if self.presenting {
            return self.present(false);
        }
        Task::none()
    }

    /// Give the backlight back even when the window is closed mid-presentation.
    ///
    /// There is no executor left to await at this point, so this is the one
    /// place the blocking path is used.
    fn on_app_exit(&mut self) -> Option<Self::Message> {
        if let Some(brightness) = self.hold.owed_brightness() {
            screen::restore_blocking(brightness);
        }
        None
    }

    fn context_drawer(&self) -> Option<context_drawer::ContextDrawer<'_, Self::Message>> {
        if !self.core.window.show_context {
            return None;
        }
        Some(context_drawer::about(
            &self.about,
            |url| Message::LaunchUrl(url.to_string()),
            Message::ToggleAbout,
        ))
    }

    fn view(&self) -> Element<'_, Self::Message> {
        let spacing = cosmic::theme::spacing();

        if let Some(why) = &self.fatal {
            return widget::text::body(why.clone()).into();
        }

        if self.presenting
            && let Some((pass, symbol)) = self.presentable()
        {
            return crate::presenter::view(pass, symbol);
        }

        let mut left = widget::column::with_capacity(4)
            .spacing(spacing.space_xs)
            .push(widget::text::caption(fl!(
                "passes-count",
                count = self.passes.len()
            )));
        if self.passes.is_empty() {
            left = left.push(widget::text::body(fl!(
                "no-passes-detail",
                path = self.root.clone()
            )));
        }
        if !self.unreadable.is_empty() {
            left = left.push(widget::text::caption(fl!(
                "unreadable-passes",
                count = self.unreadable.len()
            )));
            for line in self.unreadable_lines() {
                left = left.push(widget::text::caption(line));
            }
        }
        left = left.push(self.list());

        widget::row::with_capacity(3)
            .spacing(spacing.space_s)
            .padding(spacing.space_s)
            .push(left.width(Length::FillPortion(2)))
            .push(widget::divider::vertical::default())
            .push(
                widget::container(self.detail())
                    .width(Length::FillPortion(3))
                    .height(Length::Fill),
            )
            .into()
    }

    fn update(&mut self, message: Self::Message) -> Task<Self::Message> {
        match message {
            Message::Select(index) => self.select(index),
            Message::Present => {
                if self.presentable().is_some() {
                    return self.present(true);
                }
            }
            Message::Leave => return self.present(false),
            // What the desktop lent arrives asynchronously, so it may arrive
            // when it is no longer wanted: after the presenter closed, or on
            // top of what a newer presentation already holds. Only a hold
            // for the presenter that is up, with nothing held yet, is kept;
            // anything else is given straight back. A release reports an
            // empty hold, which is never kept over a real one.
            Message::ScreenHeld(hold) => {
                if self.presenting && self.hold.is_empty() {
                    self.hold = hold;
                } else if !hold.is_empty() {
                    return cosmic::task::future(async move {
                        screen::release(hold).await;
                        Message::ScreenHeld(Hold::default())
                    });
                }
            }
            Message::ToggleAbout => {
                self.core.window.show_context = !self.core.window.show_context;
            }
            Message::LaunchUrl(url) => {
                if let Err(why) = open::that_detached(&url) {
                    tracing::warn!(%why, url, "cannot open the link");
                }
            }
        }
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmic::Application as _;

    fn app() -> AppModel {
        AppModel {
            core: Core::default(),
            about: About::default(),
            nav: nav_bar::Model::default(),
            passes: Vec::new(),
            unreadable: Vec::new(),
            opened: 0,
            selected: None,
            symbol: None,
            presenting: false,
            hold: Hold::default(),
            root: String::new(),
            fatal: None,
        }
    }

    /// Present, then Done before the portal and the settings daemon have
    /// answered: what they lend afterwards has to go straight back, not be
    /// kept as if a barcode were still on screen.
    #[test]
    fn a_hold_that_arrives_after_the_presenter_closed_is_given_back() {
        let mut app = app();
        let _ = app.update(Message::ScreenHeld(Hold::owing(40)));
        assert_eq!(
            app.hold.owed_brightness(),
            None,
            "the raised backlight was kept after the presenter closed"
        );
    }

    /// Leave and present again quickly: the first release finishing late must
    /// not overwrite what the second presentation borrowed, or the backlight
    /// is never given back.
    #[test]
    fn a_release_finishing_late_keeps_the_new_presentations_hold() {
        let mut app = app();
        app.presenting = true;
        let _ = app.update(Message::ScreenHeld(Hold::owing(40)));
        let _ = app.update(Message::ScreenHeld(Hold::default()));
        assert_eq!(app.hold.owed_brightness(), Some(40));
    }

    /// A pass that will not read is listed by its folder and the reason, not
    /// only counted.
    #[test]
    fn an_unreadable_pass_is_named_with_its_reason() {
        let mut app = app();
        app.unreadable.push(UnreadablePass {
            id: "flight-to-lhr".to_owned(),
            reason: "not a zip archive".to_owned(),
        });
        let lines = app.unreadable_lines();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("flight-to-lhr") && lines[0].contains("not a zip archive"));
    }

    /// A pass with the given dates, and nothing else of note.
    fn pass(expiry: Option<&str>, voided: bool) -> Pass {
        Pass {
            kind: PassKind::BoardingPass,
            serial_number: "S".to_owned(),
            pass_type_identifier: String::new(),
            team_identifier: String::new(),
            organization_name: "Example Air".to_owned(),
            description: String::new(),
            logo_text: None,
            transit_type: None,
            relevant_date: None,
            expiration_date: expiry.map(|date| chrono::DateTime::parse_from_rfc3339(date).unwrap()),
            voided,
            web_service_url: None,
            header_fields: Vec::new(),
            primary_fields: vec![pocket_core::Field {
                key: "route".to_owned(),
                label: None,
                value: "ATH".to_owned(),
            }],
            secondary_fields: Vec::new(),
            auxiliary_fields: Vec::new(),
            back_fields: Vec::new(),
            barcodes: Vec::new(),
            background_color: None,
            foreground_color: None,
            label_color: None,
        }
    }

    /// An expired or voided pass says so in the list; a good one does not.
    #[test]
    fn the_list_says_when_a_pass_is_expired_or_voided() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-09-29T12:00:00+00:00").unwrap();
        let good = summary(&pass(Some("2026-10-01T00:00:00+00:00"), false), now).unwrap();
        let expired = summary(&pass(Some("2026-09-01T00:00:00+00:00"), false), now).unwrap();
        let voided = summary(&pass(None, true), now).unwrap();
        assert!(!good.contains(&fl!("expired")) && !good.contains(&fl!("voided")));
        assert!(expired.contains(&fl!("expired")), "{expired}");
        assert!(voided.contains(&fl!("voided")), "{voided}");
    }

    /// "Open with Pocket" on a `.pkpass` shows that pass; a file that is not
    /// one is named as unreadable rather than ignored.
    #[test]
    fn files_opened_from_the_file_manager_are_shown_or_reported() {
        let dir = tempfile::tempdir().unwrap();
        let broken = dir.path().join("broken.pkpass");
        std::fs::write(&broken, b"not a zip").unwrap();
        let missing = dir.path().join("missing.pkpass");

        let good = dir.path().join("flight.pkpass");
        std::fs::write(
            &good,
            pkpass(r#"{"organizationName":"Example Air","boardingPass":{}}"#),
        )
        .unwrap();

        let (passes, unreadable) = open_files(&[good.clone(), broken, missing]);
        assert_eq!(passes.len(), 1);
        assert_eq!(passes[0].path, good);
        assert_eq!(passes[0].pass.title(), "Example Air");
        let ids: Vec<&str> = unreadable.iter().map(|u| u.id.as_str()).collect();
        assert_eq!(ids, ["broken.pkpass", "missing.pkpass"]);
    }

    /// A `.pkpass` holding `pass_json`, with a manifest the reader accepts.
    fn pkpass(pass_json: &str) -> Vec<u8> {
        use sha1::Digest as _;
        use std::io::Write as _;

        let digest: String = sha1::Sha1::digest(pass_json.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let manifest = format!(r#"{{"pass.json":"{digest}"}}"#);
        let mut buffer = Vec::new();
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buffer));
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("pass.json", options).unwrap();
        zip.write_all(pass_json.as_bytes()).unwrap();
        zip.start_file("manifest.json", options).unwrap();
        zip.write_all(manifest.as_bytes()).unwrap();
        zip.finish().unwrap();
        buffer
    }
}
