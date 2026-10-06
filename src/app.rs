// SPDX-License-Identifier: GPL-3.0-only

//! The Pocket application shell.
//!
//! Deliberately thin. It lists what the store holds, filters by style, draws
//! the selected pass, and puts its barcode full-screen when asked. Everything
//! about *what a pass is* lives in `pocket-core`; how a pass looks is in
//! [`crate::face`] and [`crate::presenter`].
//!
//! # Shown, and kept
//!
//! A `.pkpass` reaches the window two ways. **Opened** — "Open with Pocket"
//! in a file manager, or `pocket flight.pkpass` — it is *shown*: at the top
//! of the list, with its barcode ready, and nothing is written anywhere.
//! Keeping it is a separate, explicit step, **Add to wallet**, because
//! looking at a pass somebody sent is not a decision to carry it. **Add
//! pass…** in the header, or dropping files on the window, is that decision
//! made up front: those files go straight into the wallet. A `.pkpasses`
//! bundle — several travellers on one booking — is each pass it holds.
//!
//! Either way it is `PassStore::add` that stores the pass — verified first,
//! written atomically, and one pass however often it is added.

use std::path::{Path, PathBuf};

use cosmic::app::{Core, Task, context_drawer};
use cosmic::iced::Length;
use cosmic::prelude::*;
use cosmic::widget::about::About;
use cosmic::widget::{self, nav_bar};
use pocket_core::{Added, Listing, Pass, PassKind, PassStore, StoredPass, Symbol, UnreadablePass};

use crate::fl;
use crate::launch::{self, Flags};
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
    /// Keep the selected pass, which was opened from a file, in the wallet.
    Add,
    /// Choose `.pkpass` files to add to the wallet.
    ChooseFiles,
    /// Add these files to the wallet: chosen in the dialog, or dropped on
    /// the window. None when the dialog was dismissed.
    Import(Vec<PathBuf>),
    /// A drag carrying files is over the window, or has left it.
    Dragging(bool),
    /// Files were dropped on the window, as a `text/uri-list`.
    Dropped(Vec<u8>),
    /// Files were dropped by a sandboxed application, which hands over a
    /// document-portal key in place of paths.
    DroppedThroughPortal(String),
    /// Ask before removing the selected pass from the wallet.
    AskRemove,
    /// Remove it.
    Remove,
    /// Leave it.
    CancelRemove,
    /// Something to say that is not about one pass: the file dialog failed.
    Notice(String),
}

pub struct AppModel {
    core: Core,
    about: About,
    nav: nav_bar::Model,
    /// The wallet. `None` when it could not be opened; `fatal` says why.
    store: Option<PassStore>,
    /// The passes opened from files, then everything the store holds in its
    /// own order. The sidebar filters this rather than reloading, so switching
    /// rows costs no disk read.
    passes: Vec<StoredPass>,
    /// Passes in the store that would not parse. Surfaced rather than dropped
    /// — a wallet that silently shows fewer passes than the directory holds
    /// is the failure a traveller finds at the gate.
    unreadable: Vec<UnreadablePass>,
    /// Files that were opened, or chosen to be added, and would not read: by
    /// name, with the reason. A file the user pointed at must not vanish
    /// without a word.
    unreadable_files: Vec<UnreadablePass>,
    /// The bytes of each pass shown from a file, in the order they lead
    /// `passes`. Kept so that Add to wallet stores exactly what was shown.
    shown: Vec<Vec<u8>>,
    /// A drag carrying files is over the window.
    dragging: bool,
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
    /// What the last change to the wallet did, or why it did not happen.
    notice: Option<String>,
    /// The stored pass the user has been asked about removing, by its id.
    removing: Option<String>,
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
        self.unreadable_files
            .iter()
            .chain(&self.unreadable)
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
        let spacing = cosmic::theme::spacing();
        let face = crate::face::view(pass, self.symbol.as_ref(), now());
        // Above a pass that is only being shown: that it is not kept, and the
        // way to keep it. Below one that is kept: the way to stop.
        if self.selected.is_some_and(|index| index < self.opened()) {
            return widget::column::with_capacity(2)
                .spacing(spacing.space_xs)
                .push(
                    widget::row::with_capacity(2)
                        .spacing(spacing.space_s)
                        .align_y(cosmic::iced::Alignment::Center)
                        .push(widget::text::caption(fl!("opened-from-file")).width(Length::Fill))
                        .push(
                            widget::button::suggested(fl!("add-to-wallet")).on_press(Message::Add),
                        ),
                )
                .push(face)
                .into();
        }
        widget::column::with_capacity(2)
            .spacing(spacing.space_xs)
            .push(face)
            .push(
                widget::button::destructive(fl!("remove-from-wallet")).on_press(Message::AskRemove),
            )
            .into()
    }

    /// How many of `passes`, from the front, are only being shown.
    fn opened(&self) -> usize {
        self.shown.len()
    }

    /// The selected pass, when it is one the store holds.
    fn selected_stored(&self) -> Option<&StoredPass> {
        self.selected
            .filter(|index| *index >= self.opened())
            .and_then(|index| self.passes.get(index))
    }

    /// Selects the stored pass at `path`, or nothing when there is none.
    fn select_stored(&mut self, path: Option<&Path>) {
        let index = path.and_then(|path| {
            self.passes
                .iter()
                .skip(self.opened())
                .position(|stored| stored.path == path)
                .map(|index| index + self.opened())
        });
        match index {
            Some(index) => self.select(index),
            None => {
                self.selected = None;
                self.symbol = None;
            }
        }
    }

    /// Reads the store again, keeping the passes being shown and the
    /// selection.
    fn reload(&mut self) {
        let opened = self.opened();
        let kept = self
            .selected
            .filter(|index| *index >= opened)
            .and_then(|index| self.passes.get(index))
            .map(|stored| stored.path.clone());
        let Listing { passes, unreadable } =
            self.store.as_ref().map_or_else(Listing::default, list);
        self.passes.truncate(opened);
        self.passes.extend(passes);
        self.unreadable = unreadable;
        if self.selected.is_some_and(|index| index >= opened) {
            self.select_stored(kept.as_deref());
        }
    }

    /// Shows the passes in `files` without keeping them — a `.pkpasses`
    /// bundle shows each pass it holds — and, when `select` is set, selects
    /// the first: it is what the user just asked to see.
    ///
    /// A pass already being shown is selected rather than shown twice, and a
    /// file or bundled pass that will not read is named with the reason.
    fn show(&mut self, files: &[PathBuf], select: bool) {
        let mut arrived: Vec<(StoredPass, Vec<u8>)> = Vec::new();
        let mut again = None;
        for file in files {
            let (passes, unreadable) = open_file(file);
            self.unreadable_files.extend(unreadable);
            for (stored, bytes) in passes {
                if let Some(index) = self.shown.iter().position(|shown| *shown == bytes) {
                    again.get_or_insert(index);
                } else if !arrived.iter().any(|(_, other)| *other == bytes) {
                    arrived.push((stored, bytes));
                }
            }
        }

        let count = arrived.len();
        for (stored, bytes) in arrived.into_iter().rev() {
            self.passes.insert(0, stored);
            self.shown.insert(0, bytes);
        }
        // Everything already listed moved down by what arrived.
        self.selected = self.selected.map(|index| index + count);
        if !select {
            return;
        }
        if count > 0 {
            self.select(0);
        } else if let Some(index) = again {
            self.select(index);
        }
    }

    /// Stores each of `passes` in the wallet, saying for each what happened.
    fn keep(&self, passes: &[Vec<u8>]) -> Vec<Result<(StoredPass, Added), String>> {
        let Some(store) = &self.store else {
            return vec![Err(fl!("no-wallet"))];
        };
        passes
            .iter()
            .map(|bytes| store.add(bytes).map_err(|why| why.to_string()))
            .collect()
    }

    /// Stops showing the file a pass was opened from, now that the wallet
    /// holds that pass: it is one row in the list, not the file and the
    /// wallet's copy both.
    fn forget_shown(&mut self, bytes: &[u8]) {
        let Some(index) = self.shown.iter().position(|shown| shown == bytes) else {
            return;
        };
        self.shown.remove(index);
        self.passes.remove(index);
        self.selected = match self.selected {
            Some(selected) if selected == index => None,
            Some(selected) if selected > index => Some(selected - 1),
            selected => selected,
        };
    }

    /// Adds the selected pass, which is being shown from a file, to the
    /// wallet.
    ///
    /// What is stored is what was on screen, byte for byte, even if the file
    /// has changed or gone since. Once kept it is the stored pass that is
    /// shown and selected.
    fn add_selected(&mut self) {
        let Some(index) = self.selected.filter(|index| *index < self.opened()) else {
            return;
        };
        let bytes = self.shown[index].clone();
        let name = self.passes[index].id.clone();
        match self.keep(std::slice::from_ref(&bytes)).remove(0) {
            Ok((stored, outcome)) => {
                self.forget_shown(&bytes);
                self.reload();
                self.select_stored(Some(&stored.path));
                self.notice = Some(added(outcome));
            }
            Err(reason) => {
                self.notice = Some(fl!("add-failed", name = name, reason = reason));
            }
        }
    }

    /// Adds files to the wallet — chosen in the dialog or dropped on the
    /// window — and selects the last pass added. A `.pkpasses` bundle adds
    /// every pass it holds. What cannot be added is named with the reason and
    /// does not stop the rest. A pass that was being shown from a file is
    /// from then on the wallet's.
    fn import(&mut self, files: &[PathBuf]) {
        if files.is_empty() {
            return;
        }
        let mut last = None;
        let mut outcomes = Vec::new();
        for file in files {
            let passes = read_file(file).and_then(|bytes| {
                pocket_core::pkpass::split(&bytes).map_err(|why| why.to_string())
            });
            let passes = match passes {
                Ok(passes) => passes,
                Err(reason) => {
                    self.unreadable_files.push(UnreadablePass {
                        id: name_of(file),
                        reason,
                    });
                    continue;
                }
            };
            let bundled = passes.len() > 1;
            for (index, result) in self.keep(&passes).into_iter().enumerate() {
                match result {
                    Ok((stored, outcome)) => {
                        self.forget_shown(&passes[index]);
                        last = Some(stored.path);
                        outcomes.push(outcome);
                    }
                    Err(reason) => self.unreadable_files.push(UnreadablePass {
                        id: entry_name(file, bundled.then_some(index)),
                        reason,
                    }),
                }
            }
        }
        self.reload();
        if last.is_some() {
            self.select_stored(last.as_deref());
        }
        self.notice = match outcomes[..] {
            [] => None,
            [outcome] => Some(added(outcome)),
            _ => Some(added_several(&outcomes)),
        };
    }

    /// Removes the pass the user was asked about from the wallet.
    fn remove(&mut self) {
        let Some(id) = self.removing.take() else {
            return;
        };
        let Some(store) = &self.store else {
            return;
        };
        let name = self
            .passes
            .iter()
            .skip(self.opened())
            .find(|stored| stored.id == id)
            .map_or_else(|| id.clone(), |stored| stored.pass.title().to_owned());
        self.notice = Some(match store.remove(&id) {
            Ok(()) => fl!("removed", name = name),
            Err(why) => fl!("remove-failed", name = name, reason = why.to_string()),
        });
        self.reload();
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

/// The bytes of a file somebody pointed at, up to [`OPEN_FILE_LIMIT`].
fn read_file(path: &Path) -> Result<Vec<u8>, String> {
    use std::io::Read as _;

    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(OPEN_FILE_LIMIT).read_to_end(&mut bytes))
        .map_err(|why| why.to_string())?;
    Ok(bytes)
}

/// What a file is called in the list: its name, not the path to it.
fn name_of(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// What a pass from `file` is called in the list: the file's name, and for
/// a pass out of a bundle, which one.
fn entry_name(file: &Path, bundled: Option<usize>) -> String {
    match bundled {
        Some(index) => format!("{} ({})", name_of(file), index + 1),
        None => name_of(file),
    }
}

/// Reads a `.pkpass` or `.pkpasses` file for showing: each pass it holds,
/// with the bytes it was read from, and what would not read.
///
/// A file the user explicitly opened must not vanish without a word, and one
/// broken pass in a bundle does not hide the others.
fn open_file(path: &Path) -> (Vec<(StoredPass, Vec<u8>)>, Vec<UnreadablePass>) {
    let passes = match read_file(path)
        .and_then(|bytes| pocket_core::pkpass::split(&bytes).map_err(|why| why.to_string()))
    {
        Ok(passes) => passes,
        Err(reason) => {
            return (
                Vec::new(),
                vec![UnreadablePass {
                    id: name_of(path),
                    reason,
                }],
            );
        }
    };
    let bundled = passes.len() > 1;
    let mut shown = Vec::new();
    let mut unreadable = Vec::new();
    for (index, bytes) in passes.into_iter().enumerate() {
        let id = entry_name(path, bundled.then_some(index));
        match pocket_core::pkpass::read(&bytes) {
            Ok(pass) => shown.push((
                StoredPass {
                    id,
                    path: path.to_path_buf(),
                    pass,
                },
                bytes,
            )),
            Err(why) => unreadable.push(UnreadablePass {
                id,
                reason: why.to_string(),
            }),
        }
    }
    (shown, unreadable)
}

/// The local files named in a `text/uri-list`, which is what a file manager
/// puts on a drag.
///
/// Lines starting with `#` are comments (RFC 2483), and anything that is not
/// a `file:` URL is left out: a pass dragged from a web page is a link, and
/// fetching it is not something a drop should do unasked.
fn paths_in_uri_list(data: &[u8]) -> Vec<PathBuf> {
    String::from_utf8_lossy(data)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| url::Url::parse(line).ok())
        .filter_map(|url| url.to_file_path().ok())
        .collect()
}

/// Everything the store holds, or nothing when it cannot be listed.
fn list(store: &PassStore) -> Listing {
    match store.list() {
        Ok(listing) => listing,
        Err(why) => {
            tracing::error!(%why, "cannot list passes");
            Listing::default()
        }
    }
}

/// What adding a pass did, in words.
fn added(outcome: Added) -> String {
    match outcome {
        Added::New => fl!("added"),
        Added::Updated => fl!("added-updated"),
        Added::Unchanged => fl!("added-already"),
    }
}

/// What adding several passes at once did: how many were new, how many
/// replaced an earlier version, and how many the wallet already had — a pass
/// it already had was not added, and saying it was would be wrong.
fn added_several(outcomes: &[Added]) -> String {
    let count = |which: Added| outcomes.iter().filter(|outcome| **outcome == which).count();
    let (new, updated, unchanged) = (
        count(Added::New),
        count(Added::Updated),
        count(Added::Unchanged),
    );
    let mut sentences = Vec::with_capacity(3);
    if new > 0 {
        sentences.push(fl!("added-several", count = new));
    }
    if updated > 0 {
        sentences.push(fl!("updated-several", count = updated));
    }
    if unchanged > 0 {
        sentences.push(fl!("already-several", count = unchanged));
    }
    sentences.join(" ")
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
    type Flags = Flags;
    type Message = Message;
    const APP_ID: &'static str = APP_ID;

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, flags: Self::Flags) -> (Self, Task<Self::Message>) {
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

        let mut nav = nav_bar::Model::default();
        nav.insert()
            .text(label(Filter::All))
            .data(Filter::All)
            .activate();
        for kind in PassKind::all() {
            let filter = Filter::Kind(kind);
            nav.insert().text(label(filter)).data(filter);
        }

        let mut app = Self {
            core,
            about,
            nav,
            store,
            passes: Vec::new(),
            unreadable: Vec::new(),
            unreadable_files: Vec::new(),
            shown: Vec::new(),
            dragging: false,
            selected: None,
            symbol: None,
            presenting: false,
            hold: Hold::default(),
            root,
            fatal,
            notice: None,
            removing: None,
        };
        app.reload();
        // Files handed over by the file manager come first and the first of
        // them is selected: it is what the user just asked to see.
        app.show(&flags.paths(), true);
        (app, Task::none())
    }

    /// A second launch handing over the files it was asked to open — "Open
    /// with Pocket" on another pass while this window is up.
    ///
    /// The window has already been raised by the time this is called; what is
    /// left is to show the passes, exactly as the first launch would have.
    fn dbus_activation(
        &mut self,
        message: cosmic::dbus_activation::Message,
    ) -> Task<Self::Message> {
        use cosmic::dbus_activation::Details;

        let files = match message.msg {
            Details::ActivateAction { action, args } if action == launch::OPEN => {
                launch::paths(&args)
            }
            // A launcher that speaks the interface itself sends the files as
            // URLs rather than through a second `pocket`.
            Details::Open { url } => url
                .iter()
                .filter_map(|url| url.to_file_path().ok())
                .collect(),
            // Nothing to show, or an action Pocket does not have: raising the
            // window was the whole request.
            Details::Activate | Details::ActivateAction { .. } => Vec::new(),
        };
        // The barcode on screen is what the user is in the middle of; a pass
        // arriving from elsewhere joins the list without taking its place.
        self.show(&files, !self.presenting);
        Task::none()
    }

    /// The one action the header offers: adding passes to the wallet.
    fn header_start(&self) -> Vec<Element<'_, Self::Message>> {
        if self.fatal.is_some() || self.presenting {
            return Vec::new();
        }
        vec![
            widget::button::text(fl!("add-pass"))
                .leading_icon(widget::icon::from_name("list-add-symbolic").size(16))
                .on_press(Message::ChooseFiles)
                .into(),
        ]
    }

    /// Removing a pass deletes a file that may exist nowhere else, so it is
    /// asked about first.
    fn dialog(&self) -> Option<Element<'_, Self::Message>> {
        let id = self.removing.as_ref()?;
        let stored = self
            .passes
            .iter()
            .skip(self.opened())
            .find(|stored| &stored.id == id)?;
        Some(
            widget::dialog()
                .title(fl!("remove-title", name = stored.pass.title().to_owned()))
                .body(fl!("remove-body"))
                .primary_action(
                    widget::button::destructive(fl!("remove")).on_press(Message::Remove),
                )
                .secondary_action(
                    widget::button::text(fl!("cancel")).on_press(Message::CancelRemove),
                )
                .into(),
        )
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

        let mut left = widget::column::with_capacity(5)
            .spacing(spacing.space_xs)
            .push(widget::text::caption(fl!(
                "passes-count",
                count = self.passes.len()
            )));
        if self.dragging {
            left = left.push(widget::text::body(fl!("drop-to-add")));
        } else if let Some(notice) = &self.notice {
            left = left.push(widget::text::body(notice.clone()));
        }
        if self.passes.is_empty() {
            left = left.push(widget::text::body(fl!(
                "no-passes-detail",
                path = self.root.clone()
            )));
        }
        let unreadable = self.unreadable.len() + self.unreadable_files.len();
        if unreadable > 0 {
            left = left.push(widget::text::caption(fl!(
                "unreadable-passes",
                count = unreadable
            )));
            for line in self.unreadable_lines() {
                left = left.push(widget::text::caption(line));
            }
        }
        left = left.push(self.list());

        let content = widget::row::with_capacity(3)
            .spacing(spacing.space_s)
            .padding(spacing.space_s)
            .push(left.width(Length::FillPortion(2)))
            .push(widget::divider::vertical::default())
            .push(
                widget::container(self.detail())
                    .width(Length::FillPortion(3))
                    .height(Length::Fill),
            );
        // The whole window takes a drop: a wallet has one thing to do with a
        // file, so there is no wrong place to let go of it. A sandboxed
        // application hands over a portal key rather than paths.
        widget::dnd_destination(content, vec![std::borrow::Cow::Borrowed("text/uri-list")])
            .on_enter(|_, _, _| Message::Dragging(true))
            .on_leave(|| Message::Dragging(false))
            .on_finish(|_mime, data, _action, _, _| Message::Dropped(data))
            .on_file_transfer(Message::DroppedThroughPortal)
            .into()
    }

    fn update(&mut self, message: Self::Message) -> Task<Self::Message> {
        match message {
            Message::Select(index) => {
                // What the last change did has been read by now.
                self.notice = None;
                self.select(index);
            }
            Message::Add => self.add_selected(),
            Message::ChooseFiles => {
                return cosmic::task::future(async {
                    use cosmic::dialog::file_chooser::{self, FileFilter};

                    let dialog = file_chooser::open::Dialog::new()
                        .title(fl!("add-pass"))
                        .filter(
                            FileFilter::new(&fl!("pass-files"))
                                .glob("*.pkpass")
                                .glob("*.pkpasses"),
                        );
                    match dialog.open_files().await {
                        Ok(response) => Message::Import(
                            response
                                .urls()
                                .iter()
                                .filter_map(|url| url.to_file_path().ok())
                                .collect(),
                        ),
                        // A dismissed dialog is an answer, not a failure.
                        Err(file_chooser::Error::Cancelled) => Message::Import(Vec::new()),
                        Err(why) => Message::Notice(why.to_string()),
                    }
                });
            }
            Message::Import(files) => self.import(&files),
            Message::Dragging(dragging) => self.dragging = dragging,
            Message::Dropped(data) => {
                self.dragging = false;
                self.import(&paths_in_uri_list(&data));
            }
            Message::DroppedThroughPortal(key) => {
                self.dragging = false;
                return cosmic::command::file_transfer_receive(key).map(|received| {
                    cosmic::Action::App(match received {
                        Ok(files) => {
                            Message::Import(files.into_iter().map(PathBuf::from).collect())
                        }
                        Err(why) => Message::Notice(why.to_string()),
                    })
                });
            }
            Message::AskRemove => {
                self.removing = self.selected_stored().map(|stored| stored.id.clone());
            }
            Message::Remove => self.remove(),
            Message::CancelRemove => self.removing = None,
            Message::Notice(notice) => self.notice = Some(notice),
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
            store: None,
            passes: Vec::new(),
            unreadable: Vec::new(),
            unreadable_files: Vec::new(),
            shown: Vec::new(),
            dragging: false,
            selected: None,
            symbol: None,
            presenting: false,
            hold: Hold::default(),
            root: String::new(),
            fatal: None,
            notice: None,
            removing: None,
        }
    }

    /// An application over a wallet in `dir`, which starts empty.
    fn app_with_wallet(dir: &Path) -> AppModel {
        let mut app = app();
        app.store = Some(PassStore::open(dir.join("passes")));
        app.reload();
        app
    }

    /// A boarding pass file in `dir`, as a file manager would hand it over.
    fn pass_file(dir: &Path, name: &str, serial: &str, gate: &str) -> PathBuf {
        let path = dir.join(name);
        let json = format!(
            r#"{{"passTypeIdentifier":"pass.com.example.air","serialNumber":"{serial}",
                "organizationName":"Example Air","logoText":"Gate {gate}","boardingPass":{{}}}}"#
        );
        std::fs::write(&path, pkpass(&json)).unwrap();
        path
    }

    /// What a second `pocket <files>` sends the running window.
    fn handed_over(files: &[&Path]) -> cosmic::dbus_activation::Message {
        use cosmic::app::CosmicFlags as _;

        let flags = Flags::new(files.iter().map(|file| file.as_os_str().to_owned()));
        cosmic::dbus_activation::Message {
            activation_token: None,
            desktop_startup_id: None,
            msg: cosmic::dbus_activation::Details::ActivateAction {
                action: flags.action().expect("there is something to show").clone(),
                args: flags.args().into_iter().map(str::to_owned).collect(),
            },
        }
    }

    /// "Open with Pocket" on a second pass while Pocket is running: the pass
    /// is shown in the window that is up, selected, with its barcode ready —
    /// and the pass that was being looked at is still in the list.
    #[test]
    fn a_pass_opened_while_running_is_shown_in_this_window() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_wallet(dir.path());
        let first = pass_file(dir.path(), "first.pkpass", "A1", "1");
        app.show(std::slice::from_ref(&first), true);
        assert_eq!(app.opened(), 1);

        let second = pass_file(dir.path(), "second.pkpass", "B2", "2");
        let _ = app.dbus_activation(handed_over(&[&second]));
        assert_eq!(app.opened(), 2);
        assert_eq!(app.passes[app.selected.unwrap()].path, second);
        assert!(app.passes.iter().any(|stored| stored.path == first));

        // Opened again: selected, not shown twice.
        let _ = app.dbus_activation(handed_over(&[&first]));
        assert_eq!(app.opened(), 2);
        assert_eq!(app.passes[app.selected.unwrap()].path, first);
    }

    /// A launch with nothing to show raises the window and changes nothing
    /// in it; a file that will not read is named with the reason.
    #[test]
    fn a_handover_with_nothing_readable_shows_no_pass() {
        use cosmic::dbus_activation::{Details, Message as Activation};

        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_wallet(dir.path());
        let _ = app.dbus_activation(Activation {
            activation_token: None,
            desktop_startup_id: None,
            msg: Details::Activate,
        });
        assert!(app.passes.is_empty() && app.unreadable_files.is_empty());

        let broken = dir.path().join("broken.pkpass");
        std::fs::write(&broken, b"not a zip").unwrap();
        let _ = app.dbus_activation(handed_over(&[&broken]));
        assert!(app.passes.is_empty());
        assert_eq!(app.unreadable_files.len(), 1);
        assert_eq!(app.unreadable_files[0].id, "broken.pkpass");
    }

    /// A pass arriving while a barcode is on the whole screen joins the list
    /// without taking the presenter away from the pass being scanned.
    #[test]
    fn a_pass_opened_during_a_presentation_does_not_replace_the_barcode() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_wallet(dir.path());
        let presented = pass_file(dir.path(), "presented.pkpass", "A1", "1");
        app.show(std::slice::from_ref(&presented), true);
        app.presenting = true;

        let other = pass_file(dir.path(), "other.pkpass", "B2", "2");
        let _ = app.dbus_activation(handed_over(&[&other]));
        assert_eq!(app.opened(), 2);
        assert_eq!(app.passes[app.selected.unwrap()].path, presented);
    }

    /// Add to wallet: the pass that was only being shown is stored, and what
    /// is selected afterwards is the stored pass, not the file. Adding it a
    /// second time leaves one pass in the wallet.
    #[test]
    fn a_pass_opened_from_a_file_is_kept_once_it_is_added() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_wallet(dir.path());
        let file = pass_file(dir.path(), "flight.pkpass", "A1", "1");
        app.show(std::slice::from_ref(&file), true);

        let _ = app.update(Message::Add);
        assert_eq!(app.opened(), 0, "the file is still shown beside the pass");
        assert_eq!(app.passes.len(), 1);
        let kept = &app.passes[app.selected.expect("the added pass is selected")];
        assert!(kept.path.starts_with(dir.path().join("passes")));
        assert_eq!(
            std::fs::read(&kept.path).unwrap(),
            std::fs::read(&file).unwrap(),
            "the pass was not stored byte for byte"
        );
        assert_eq!(app.notice, Some(fl!("added")));

        app.show(std::slice::from_ref(&file), true);
        let _ = app.update(Message::Add);
        assert_eq!(app.passes.len(), 1, "the same pass is in the wallet twice");
        assert_eq!(app.notice, Some(fl!("added-already")));

        // The issuer's re-send, with a new gate: the same pass, replaced.
        let resent = pass_file(dir.path(), "resent.pkpass", "A1", "7");
        app.show(std::slice::from_ref(&resent), true);
        let _ = app.update(Message::Add);
        assert_eq!(app.passes.len(), 1);
        assert_eq!(app.passes[0].pass.logo_text.as_deref(), Some("Gate 7"));
        assert_eq!(app.notice, Some(fl!("added-updated")));
    }

    /// What Add to wallet keeps is what was shown, even when the file has
    /// changed since; a pass the wallet cannot take says why and stays on
    /// show.
    #[test]
    fn add_keeps_the_pass_that_was_shown_and_says_when_it_cannot() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_wallet(dir.path());
        let file = pass_file(dir.path(), "flight.pkpass", "A1", "1");
        let shown = std::fs::read(&file).unwrap();
        app.show(std::slice::from_ref(&file), true);
        std::fs::write(&file, b"no longer a pass").unwrap();

        let _ = app.update(Message::Add);
        assert_eq!(app.opened(), 0);
        let kept = &app.passes[app.selected.unwrap()];
        assert_eq!(std::fs::read(&kept.path).unwrap(), shown);

        // A wallet that cannot be written to: its folder is a file.
        let blocked = dir.path().join("blocked");
        std::fs::write(&blocked, b"").unwrap();
        app.store = Some(PassStore::open(&blocked));
        let other = pass_file(dir.path(), "other.pkpass", "B2", "2");
        app.show(std::slice::from_ref(&other), true);
        let _ = app.update(Message::Add);
        assert_eq!(app.opened(), 1, "a pass that was not kept left the list");
        assert!(app.notice.as_deref().unwrap().contains("other.pkpass"));
    }

    /// A `.pkpasses` bundle — one booking, two travellers — opened from the
    /// file manager shows both passes; added, it puts both in the wallet.
    #[test]
    fn a_bundle_shows_and_adds_every_pass_in_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_wallet(dir.path());
        let bundle = dir.path().join("family.pkpasses");
        std::fs::write(
            &bundle,
            zipped(&[
                (
                    "Traveller-1.pkpass",
                    std::fs::read(pass_file(dir.path(), "a.pkpass", "A1", "1")).unwrap(),
                ),
                (
                    "Traveller-2.pkpass",
                    std::fs::read(pass_file(dir.path(), "b.pkpass", "A2", "1")).unwrap(),
                ),
            ]),
        )
        .unwrap();

        app.show(std::slice::from_ref(&bundle), true);
        assert_eq!(app.opened(), 2);
        let names: Vec<&str> = app.passes.iter().map(|stored| stored.id.as_str()).collect();
        assert_eq!(names, ["family.pkpasses (1)", "family.pkpasses (2)"]);

        let _ = app.update(Message::Import(vec![bundle]));
        let stored: Vec<&str> = app
            .passes
            .iter()
            .skip(app.opened())
            .map(|stored| stored.pass.serial_number.as_str())
            .collect();
        assert_eq!(stored.len(), 2);
        assert!(stored.contains(&"A1") && stored.contains(&"A2"));
        assert_eq!(app.notice, Some(fl!("added-several", count = 2)));
    }

    /// A pass being shown from a file, then added by another route — dropped
    /// on the window, or chosen in the dialog — is in the list once: the
    /// wallet's copy, not the file and the wallet's copy both. Other files
    /// being shown stay as they were.
    #[test]
    fn a_shown_pass_added_by_another_route_is_one_row() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_wallet(dir.path());
        let file = pass_file(dir.path(), "flight.pkpass", "A1", "1");
        let other = pass_file(dir.path(), "other.pkpass", "B2", "1");
        app.show(&[file.clone(), other.clone()], true);
        assert_eq!(app.opened(), 2);

        let _ = app.update(Message::Import(vec![file]));
        assert_eq!(app.opened(), 1, "the added pass is still shown as a file");
        assert_eq!(app.passes.len(), 2);
        assert_eq!(app.passes[0].path, other);
        let kept = &app.passes[app.selected.expect("the added pass is selected")];
        assert_eq!(kept.pass.serial_number, "A1");
        assert!(kept.path.starts_with(dir.path().join("passes")));
    }

    /// Files dropped on the window go into the wallet; what the drag carried
    /// that is not a local file is left alone.
    #[test]
    fn files_dropped_on_the_window_are_added_to_the_wallet() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_wallet(dir.path());
        let file = pass_file(dir.path(), "my flight.pkpass", "A1", "1");
        let list = format!(
            "# from a file manager\r\n{}\r\nhttps://example.test/pass.pkpass\r\n",
            url::Url::from_file_path(&file).unwrap()
        );
        let _ = app.update(Message::Dragging(true));

        let _ = app.update(Message::Dropped(list.into_bytes()));
        assert!(!app.dragging, "the drop left the window waiting for one");
        assert_eq!(app.opened(), 0);
        assert_eq!(app.passes.len(), 1);
        assert_eq!(app.notice, Some(fl!("added")));
    }

    #[test]
    fn a_uri_list_yields_only_its_local_files() {
        let paths = paths_in_uri_list(
            b"# a comment\r\nfile:///tmp/a%20b.pkpass\r\n\r\nhttps://example.test/c.pkpass\r\nnot a url\r\nfile:///tmp/d.pkpasses\r\n",
        );
        assert_eq!(
            paths,
            [
                PathBuf::from("/tmp/a b.pkpass"),
                PathBuf::from("/tmp/d.pkpasses")
            ]
        );
    }

    /// "Add pass…": the files chosen go straight into the wallet; one that is
    /// not a pass is named with the reason and does not stop the others.
    #[test]
    fn passes_chosen_in_the_file_dialog_are_added_to_the_wallet() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_wallet(dir.path());
        let good = pass_file(dir.path(), "flight.pkpass", "A1", "1");
        let other = pass_file(dir.path(), "return.pkpass", "B2", "2");
        let broken = dir.path().join("broken.pkpass");
        std::fs::write(&broken, b"not a zip").unwrap();

        let _ = app.update(Message::Import(vec![good, broken, other]));
        assert_eq!(app.opened(), 0);
        assert_eq!(app.passes.len(), 2);
        assert_eq!(
            app.passes[app.selected.unwrap()].pass.serial_number,
            "B2",
            "the last pass added is the one selected"
        );
        assert_eq!(app.unreadable_files.len(), 1);
        assert_eq!(app.unreadable_files[0].id, "broken.pkpass");
        assert_eq!(app.notice, Some(fl!("added-several", count = 2)));

        // A dismissed dialog changes nothing, the notice included.
        let _ = app.update(Message::Import(Vec::new()));
        assert_eq!(app.notice, Some(fl!("added-several", count = 2)));
    }

    /// Several passes at once are counted by what happened to each: one that
    /// was already in the wallet, or that replaced an earlier version, was
    /// not "added".
    #[test]
    fn an_import_of_several_passes_says_what_happened_to_each() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_wallet(dir.path());
        let held = pass_file(dir.path(), "held.pkpass", "A1", "1");
        let regated = pass_file(dir.path(), "regated.pkpass", "B2", "1");
        let _ = app.update(Message::Import(vec![held.clone(), regated]));
        assert_eq!(app.notice, Some(fl!("added-several", count = 2)));

        let regated = pass_file(dir.path(), "regated.pkpass", "B2", "7");
        let new = pass_file(dir.path(), "new.pkpass", "C3", "1");
        let _ = app.update(Message::Import(vec![held, regated, new]));
        assert_eq!(app.passes.len(), 3);
        assert_eq!(
            app.notice,
            Some(
                [
                    fl!("added-several", count = 1),
                    fl!("updated-several", count = 1),
                    fl!("already-several", count = 1),
                ]
                .join(" ")
            )
        );
    }

    /// Removing a pass asks first. Cancel leaves it; Remove deletes it from
    /// the store and from the list, and leaves the other passes alone.
    #[test]
    fn a_pass_is_removed_from_the_wallet_only_after_being_asked_about() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_wallet(dir.path());
        let _ = app.update(Message::Import(vec![
            pass_file(dir.path(), "flight.pkpass", "A1", "1"),
            pass_file(dir.path(), "return.pkpass", "B2", "2"),
        ]));
        let doomed = app.passes[app.selected.unwrap()].clone();

        let _ = app.update(Message::AskRemove);
        assert!(
            cosmic::Application::dialog(&app).is_some(),
            "nothing was asked"
        );
        let _ = app.update(Message::CancelRemove);
        assert!(doomed.path.exists(), "a cancelled removal removed the pass");
        assert_eq!(app.passes.len(), 2);

        let _ = app.update(Message::AskRemove);
        let _ = app.update(Message::Remove);
        assert!(!doomed.path.exists());
        assert_eq!(app.passes.len(), 1);
        assert_ne!(app.passes[0].id, doomed.id);
        assert_eq!(app.selected, None);
        assert!(cosmic::Application::dialog(&app).is_none());
    }

    /// A pass that is only being shown is not the wallet's to remove: there
    /// is nothing to ask about.
    #[test]
    fn a_pass_opened_from_a_file_cannot_be_removed() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_wallet(dir.path());
        let file = pass_file(dir.path(), "flight.pkpass", "A1", "1");
        app.show(std::slice::from_ref(&file), true);

        let _ = app.update(Message::AskRemove);
        assert_eq!(app.removing, None);
        let _ = app.update(Message::Remove);
        assert!(file.exists());
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

        let mut app = app();
        app.show(&[good.clone(), broken, missing], true);
        assert_eq!(app.opened(), 1);
        assert_eq!(app.passes[0].path, good);
        assert_eq!(app.passes[0].pass.title(), "Example Air");
        assert_eq!(app.selected, Some(0), "the opened pass is not selected");
        let ids: Vec<&str> = app.unreadable_files.iter().map(|u| u.id.as_str()).collect();
        assert_eq!(ids, ["broken.pkpass", "missing.pkpass"]);
    }

    /// A zip of `files`, as a `.pkpasses` bundle is.
    fn zipped(files: &[(&str, Vec<u8>)]) -> Vec<u8> {
        use std::io::Write as _;

        let mut buffer = Vec::new();
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buffer));
        for (name, content) in files {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(content).unwrap();
        }
        zip.finish().unwrap();
        buffer
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
