// SPDX-License-Identifier: GPL-3.0-only

//! Pocket — the wallet for the COSMIC desktop.
//!
//! The data layer is not here. What a pass is, how a `.pkpass` is read, and
//! where passes live on disk are all in `pocket-core`, which has no toolkit
//! dependency so that a launcher plugin and a `peek` previewer can link it
//! too. This crate is the COSMIC front end and nothing more.
//!
//! See `ARCHITECTURE.md` for how it sits against Locket, cosmic-pim, Slate and
//! Envelope.

pub mod app;
pub mod barcode;
pub mod face;
pub mod i18n;
pub mod launch;
pub mod presenter;
pub mod screen;

/// Runs the application.
pub fn run() -> cosmic::iced::Result {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "pocket=warn".into()),
        )
        .init();

    let requested_languages = i18n_embed::DesktopLanguageRequester::requested_languages();
    i18n::init(&requested_languages);

    let settings = cosmic::app::Settings::default()
        .size(cosmic::iced::Size::new(1000.0, 720.0))
        .size_limits(
            // 360 wide is what the metainfo's `display_length` promises.
            cosmic::iced::Limits::NONE
                .min_width(360.0)
                .min_height(480.0),
        );

    // The files "Open with Pocket" hands over (`Exec=pocket %F`), shown
    // alongside the wallet without being added to it.
    let flags = launch::Flags::new(std::env::args_os().skip(1));

    // `run_single_instance` rather than `run`: when a window is already up it
    // is handed the flags over the session bus and this process exits, so a
    // second "Open with Pocket" shows its pass in the window that is there.
    cosmic::app::run_single_instance::<app::AppModel>(settings, flags)
}
