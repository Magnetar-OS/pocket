// SPDX-License-Identifier: GPL-3.0-only

//! The full-screen barcode.
//!
//! Everything on this screen exists to get a symbol read. The window is
//! white because the barcode needs the contrast, the symbol takes the space
//! that is left after one line of title and one row of controls, and there is
//! nothing else on it — no card colours, no fields, no theme. A pass has a
//! face for recognising it by; this is the other half, and the reader has no
//! opinion about typography.
//!
//! Leaving is deliberately easy: the button, or Escape. Someone at a gate
//! holding up a laptop should not have to find anything.

use cosmic::iced::{Alignment, Color, Length};
use cosmic::widget::{self, container};
use cosmic::{Element, Theme};
use pocket_core::{Pass, Symbol};

use crate::app::Message;
use crate::fl;

/// The barcode, alone, on white.
pub fn view<'a>(pass: &'a Pass, symbol: &'a Symbol) -> Element<'a, Message> {
    let spacing = cosmic::theme::spacing();

    let mut column = widget::column::with_capacity(4)
        .spacing(spacing.space_s)
        .align_x(Alignment::Center)
        .push(on_white(widget::text::heading(pass.title().to_owned())))
        .push(
            widget::container(crate::barcode::view(symbol))
                .width(Length::Fill)
                .height(Length::Fill),
        );

    if let Some(alt) = pass.barcode().and_then(|barcode| barcode.alt_text.clone()) {
        column = column.push(on_white(widget::text::body(alt)));
    }

    column = column.push(widget::button::standard(fl!("done")).on_press(Message::Leave));

    widget::container(column)
        .padding(spacing.space_m)
        .width(Length::Fill)
        .height(Length::Fill)
        .class(cosmic::theme::Container::custom(|_theme| {
            container::Style {
                background: Some(Color::WHITE.into()),
                ..Default::default()
            }
        }))
        .into()
}

/// Text that has to read against the white the barcode needs.
///
/// The theme is not consulted: in dark mode its text colour is nearly white,
/// and this surface is white whatever the desktop is set to.
fn on_white<'a>(text: widget::Text<'a, Theme>) -> widget::Text<'a, Theme> {
    text.class(cosmic::theme::Text::Color(Color::from_rgb(0.1, 0.1, 0.1)))
}
