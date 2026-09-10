// SPDX-License-Identifier: GPL-3.0-only

//! The Pocket application shell.
//!
//! Deliberately thin. It lists what the store holds, filters by style, draws
//! the selected pass, and puts its barcode full-screen when asked. Everything
//! about *what a pass is* lives in `pocket-core`; how a pass looks is in
//! [`crate::face`] and [`crate::presenter`].

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
const APP_ICON: &[u8] = include_bytes!("../resources/icons/hicolor/scalable/apps/com.magnetaros.Pocket.svg");

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
        if visible.is_empty() {
            return widget::text::body(fl!("no-passes")).into();
        }

        let mut column = widget::list_column();
        for (index, stored) in visible {
            let pass = &stored.pass;
            let mut lines = widget::column::with_capacity(2)
                .push(widget::text::body(pass.title().to_owned()))
                .spacing(2);
            if let Some(detail) = summary(pass) {
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

    fn detail(&self) -> Element<'_, Message> {
        let Some(pass) = self.selected_pass() else {
            return widget::text::body(fl!("select-a-pass")).into();
        };
        crate::face::view(pass, self.symbol.as_ref())
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

/// The one line under a pass's name in the list.
///
/// The primary fields joined, which for a boarding pass reads `ATH → LHR`
/// and for a store card is usually the single balance or member number.
fn summary(pass: &Pass) -> Option<String> {
    let values: Vec<&str> = pass
        .primary_fields
        .iter()
        .map(|field| field.value.as_str())
        .filter(|value| !value.is_empty())
        .collect();
    if values.is_empty() {
        return None;
    }
    Some(values.join(" → "))
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
    type Flags = ();
    type Message = Message;
    const APP_ID: &'static str = APP_ID;

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, _flags: Self::Flags) -> (Self, Task<Self::Message>) {
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

        let Listing { passes, unreadable } = store
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

        (
            Self {
                core,
                about,
                nav,
                passes,
                unreadable,
                selected: None,
                symbol: None,
                presenting: false,
                hold: Hold::default(),
                root,
                fatal,
            },
            Task::none(),
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
            Message::Select(index) => {
                self.selected = Some(index);
                self.symbol = self
                    .passes
                    .get(index)
                    .and_then(|stored| stored.pass.barcode())
                    .map(|barcode| {
                        pocket_core::barcode::encode(barcode).map_err(|why| why.to_string())
                    });
            }
            Message::Present => {
                if self.presentable().is_some() {
                    return self.present(true);
                }
            }
            Message::Leave => return self.present(false),
            Message::ScreenHeld(hold) => self.hold = hold,
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
