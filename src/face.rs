// SPDX-License-Identifier: GPL-3.0-only

//! The face of a pass: the issuer's colours, their logo text, and the fields
//! laid out the way the style says they are laid out.
//!
//! A pass is a designed object. The issuer chose a background colour and put
//! the flight number in the header rather than the body, and reproducing that
//! is not decoration — it is how a person recognises their own boarding pass
//! in a list of six. So the styles are not collapsed into one generic table:
//! a boarding pass shows origin and destination side by side because that is
//! what a boarding pass is.
//!
//! What the issuer does *not* get to decide is the barcode. See
//! [`crate::barcode`].

use cosmic::Element;
use cosmic::iced::{Alignment, Color, Length};
use cosmic::widget::{self, container};
use pocket_core::{Field, Pass, PassKind, Symbol, parse_color};

use crate::app::Message;
use crate::fl;

/// How much room the barcode preview gets in the detail pane, in points.
///
/// Wider than it is tall, because the widest symbology decides: a PDF417
/// boarding pass is a couple of hundred modules across, and in a square box
/// each of those modules lands on a single pixel — a picture of a barcode
/// rather than one. At this width they get two pixels each and the preview is
/// legible; the presenter is where a symbol gets the whole screen.
const PREVIEW_WIDTH: f32 = 520.0;
const PREVIEW_HEIGHT: f32 = 220.0;

/// A linear symbol needs only enough height for a reader's beam to cross it,
/// and taking the full preview box would make a loyalty card look like a
/// poster.
const LINEAR_PREVIEW_HEIGHT: f32 = 96.0;

/// What sits between a boarding pass's origin and destination.
///
/// A plain arrow rather than a vehicle per transit type: the icon themes on
/// this desktop carry no boarding-pass glyphs, and the symbol fonts that have
/// them are not reliably installed. An arrow that always renders beats a
/// mode-specific glyph that sometimes shows as a hollow box.
const TRANSIT_MARK: &str = "→";

/// The colours a pass is drawn in, with every fallback already resolved.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    /// `None` when the issuer named no background, in which case the card
    /// takes the desktop's own surface colour rather than a guess.
    pub background: Option<Color>,
    pub foreground: Option<Color>,
    pub label: Option<Color>,
}

impl Palette {
    /// Reads the issuer's colours, filling in only what can be filled in
    /// safely.
    ///
    /// A background with no foreground is the common case and the dangerous
    /// one: leaving the text at the theme's colour puts dark text on a dark
    /// card. So a foreground is derived from the background's brightness,
    /// which is the one inference here that cannot make things worse.
    #[must_use]
    pub fn of(pass: &Pass) -> Self {
        let read = |text: &Option<String>| {
            text.as_deref()
                .and_then(parse_color)
                .map(|[r, g, b]| Color::from_rgb8(r, g, b))
        };

        let background = read(&pass.background_color);
        let foreground = read(&pass.foreground_color).or_else(|| background.map(legible_on));
        let label = read(&pass.label_color).or(foreground.map(|color| Color { a: 0.75, ..color }));

        Self {
            background,
            foreground,
            label,
        }
    }

    fn text(color: Option<Color>) -> cosmic::theme::Text {
        color.map_or(cosmic::theme::Text::Default, cosmic::theme::Text::Color)
    }
}

/// Black or white, whichever the eye can read against `background`.
///
/// The coefficients are the sRGB luminance ones; the 0.55 threshold is a
/// little above the midpoint because dark text on a mid-tone reads better
/// than light text does.
fn legible_on(background: Color) -> Color {
    let luminance = 0.2126 * background.r + 0.7152 * background.g + 0.0722 * background.b;
    if luminance > 0.55 {
        Color::BLACK
    } else {
        Color::WHITE
    }
}

/// The whole of a selected pass: the card, its barcode, and its back.
///
/// `barcode` is `None` when the pass carries none, which is ordinary for a
/// coupon or a membership card and so shows nothing rather than an apology.
pub fn view<'a>(
    pass: &'a Pass,
    barcode: Option<&'a Result<Symbol, String>>,
) -> Element<'a, Message> {
    let spacing = cosmic::theme::spacing();
    let palette = Palette::of(pass);

    let mut column = widget::column::with_capacity(4)
        .spacing(spacing.space_s)
        .push(card(pass, palette));

    if let Some(barcode) = barcode {
        column = column.push(barcode_section(pass, barcode));
    }

    if !pass.back_fields.is_empty() {
        column = column
            .push(widget::divider::horizontal::default())
            .push(widget::text::heading(fl!("back-of-pass")));
        for field in &pass.back_fields {
            column = column.push(back_row(field));
        }
    }

    widget::scrollable(column).height(Length::Fill).into()
}

/// The card itself, in the issuer's colours.
fn card(pass: &Pass, palette: Palette) -> Element<'_, Message> {
    let spacing = cosmic::theme::spacing();
    let mut column = widget::column::with_capacity(5).spacing(spacing.space_s);

    column = column.push(masthead(pass, palette));

    if !pass.primary_fields.is_empty() {
        column = column.push(primary(pass, palette));
    }
    for group in [&pass.secondary_fields, &pass.auxiliary_fields] {
        if !group.is_empty() {
            column = column.push(field_row(group, palette));
        }
    }

    // A voided pass is kept and said to be void. Deleting it would be tidier
    // and would also destroy the record of something that happened.
    if pass.voided {
        column =
            column.push(widget::text::body(fl!("voided")).class(Palette::text(palette.foreground)));
    }

    widget::container(column)
        .padding(spacing.space_m)
        .width(Length::Fill)
        .class(cosmic::theme::Container::custom(move |theme| {
            let cosmic = theme.cosmic();
            container::Style {
                background: Some(
                    palette
                        .background
                        .unwrap_or_else(|| Color::from(cosmic.background(false).component.base))
                        .into(),
                ),
                text_color: palette.foreground,
                border: cosmic::iced::Border {
                    radius: cosmic.corner_radii.radius_s.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        }))
        .into()
}

/// Logo text on the left, header fields on the right — the top strip of every
/// PassKit style.
fn masthead(pass: &Pass, palette: Palette) -> Element<'_, Message> {
    let spacing = cosmic::theme::spacing();
    let mut row = widget::row::with_capacity(2)
        .spacing(spacing.space_s)
        .align_y(Alignment::Center)
        .push(
            widget::text::title4(pass.title().to_owned())
                .class(Palette::text(palette.foreground))
                .width(Length::Fill),
        );

    for field in &pass.header_fields {
        row = row.push(stacked(field, palette, Alignment::End, false));
    }
    row.into()
}

/// The big fields.
///
/// A boarding pass gets its two primaries side by side with the direction of
/// travel between them, because origin and destination are one fact rather
/// than two. Every other style has a single primary and simply shows it.
fn primary(pass: &Pass, palette: Palette) -> Element<'_, Message> {
    let spacing = cosmic::theme::spacing();

    if pass.kind == PassKind::BoardingPass && pass.primary_fields.len() >= 2 {
        return widget::row::with_capacity(3)
            .spacing(spacing.space_s)
            .align_y(Alignment::Center)
            .push(
                widget::container(stacked(
                    &pass.primary_fields[0],
                    palette,
                    Alignment::Start,
                    true,
                ))
                .width(Length::FillPortion(2)),
            )
            .push(widget::text::title3(TRANSIT_MARK).class(Palette::text(palette.label)))
            .push(
                widget::container(stacked(
                    &pass.primary_fields[1],
                    palette,
                    Alignment::End,
                    true,
                ))
                .width(Length::FillPortion(2)),
            )
            .into();
    }

    let mut column =
        widget::column::with_capacity(pass.primary_fields.len()).spacing(spacing.space_xxs);
    for field in &pass.primary_fields {
        column = column.push(stacked(field, palette, Alignment::Start, true));
    }
    column.into()
}

/// A row of secondary or auxiliary fields, sharing the width evenly.
fn field_row<'a>(fields: &'a [Field], palette: Palette) -> Element<'a, Message> {
    let spacing = cosmic::theme::spacing();
    let mut row = widget::row::with_capacity(fields.len()).spacing(spacing.space_s);
    for field in fields {
        row = row.push(
            widget::container(stacked(field, palette, Alignment::Start, false))
                .width(Length::FillPortion(1)),
        );
    }
    row.into()
}

/// One field as PassKit draws it: the label above, small, and the value below.
fn stacked<'a>(
    field: &'a Field,
    palette: Palette,
    align: Alignment,
    large: bool,
) -> Element<'a, Message> {
    let label = field.label.clone().unwrap_or_default();
    let value = if large {
        widget::text::title3(field.value.clone())
    } else {
        widget::text::body(field.value.clone())
    };

    let mut column = widget::column::with_capacity(2).align_x(align);
    if !label.is_empty() {
        column = column
            .push(widget::text::caption(label.to_uppercase()).class(Palette::text(palette.label)));
    }
    column
        .push(value.class(Palette::text(palette.foreground)))
        .into()
}

/// The barcode, or the reason there is not one.
fn barcode_section<'a>(
    pass: &'a Pass,
    barcode: &'a Result<Symbol, String>,
) -> Element<'a, Message> {
    let spacing = cosmic::theme::spacing();

    let symbol = match barcode {
        Ok(symbol) => symbol,
        // A pass whose barcode will not encode is a pass that cannot be
        // scanned, and saying so beats an empty rectangle.
        Err(why) => {
            return widget::column::with_capacity(2)
                .spacing(spacing.space_xxs)
                .push(widget::text::body(fl!("barcode-unavailable")))
                .push(widget::text::caption(why.clone()))
                .into();
        }
    };

    let height = if symbol.is_linear() {
        LINEAR_PREVIEW_HEIGHT
    } else {
        PREVIEW_HEIGHT
    };
    let drawn = widget::container(crate::barcode::view(symbol))
        .width(Length::Fill)
        .max_width(PREVIEW_WIDTH)
        .height(Length::Fixed(height))
        .padding(spacing.space_xxs)
        .class(cosmic::theme::Container::custom(|_theme| {
            container::Style {
                background: Some(Color::WHITE.into()),
                ..Default::default()
            }
        }));

    let mut column = widget::column::with_capacity(3)
        .spacing(spacing.space_xs)
        .align_x(Alignment::Center)
        .push(drawn);

    // The alternate text is the issuer's own transcription of the payload,
    // and some readers accept it typed when the screen will not scan.
    if let Some(alt) = pass.barcode().and_then(|barcode| barcode.alt_text.clone()) {
        column = column.push(widget::text::caption(alt));
    }

    column = column.push(widget::button::suggested(fl!("present")).on_press(Message::Present));

    widget::container(column).width(Length::Fill).into()
}

/// One line on the back of the pass.
fn back_row(field: &Field) -> Element<'_, Message> {
    let label = field.label.clone().unwrap_or_else(|| field.key.clone());
    widget::column::with_capacity(2)
        .spacing(2)
        .push(widget::text::caption(label))
        .push(widget::text::body(field.value.clone()))
        .into()
}
