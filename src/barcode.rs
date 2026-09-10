// SPDX-License-Identifier: GPL-3.0-only

//! Drawing a barcode so a reader can scan it off the screen.
//!
//! This is the one widget in the application with an external correctness
//! requirement. Everything else is judged by whether a person likes looking
//! at it; this is judged by whether a gate reader accepts it, and a reader is
//! unmoved by taste.
//!
//! Three rules follow from that, and each of them costs something a designer
//! would otherwise want back:
//!
//! **Whole modules only.** The module size is an integer number of pixels, so
//! every bar is the same width. Scaling a symbol to fill its box means some
//! modules land on 3 pixels and their neighbours on 4, and the ratio a reader
//! measures the code by stops being the ratio the encoder wrote. The symbol
//! is drawn at the largest whole multiple that fits and centred in what is
//! left.
//!
//! **Black on white, whatever the pass looks like.** The issuer's colours
//! belong to the card, not to the barcode; a reader needs the contrast, and
//! PassKit itself renders every barcode dark-on-light for the same reason.
//! The theme does not get a say either — a dark-mode barcode is an
//! unscannable barcode.
//!
//! **The quiet zone is drawn.** It arrives as part of the symbol from
//! `pocket_core::barcode`, so it is drawn like any other light module, and
//! the surrounding container adds more rather than less.

use cosmic::iced::{Color, Point, Rectangle, Size, mouse};
use cosmic::widget::canvas::{self, Frame, Geometry, Path};
use cosmic::{Element, Renderer, Theme};
use pocket_core::Symbol;

/// The symbol, drawn to fill the space it is given.
///
/// The caller decides the box; this decides how much of it the symbol can
/// honestly use.
pub fn view<'a, Message: 'a>(symbol: &'a Symbol) -> Element<'a, Message> {
    canvas::Canvas::new(Scannable { symbol })
        .width(cosmic::iced::Length::Fill)
        .height(cosmic::iced::Length::Fill)
        .into()
}

struct Scannable<'a> {
    symbol: &'a Symbol,
}

/// Where the symbol sits inside the space it was given, in whole modules.
struct Placement {
    module: f32,
    origin: Point,
    /// The height of one drawn row. Equal to `module` for a 2D symbol; the
    /// whole bar height for a linear one, which has only the one row.
    row_height: f32,
}

impl Scannable<'_> {
    fn place(&self, bounds: Size) -> Option<Placement> {
        let columns = self.symbol.width() as f32;
        let rows = self.symbol.height() as f32;
        if columns <= 0.0 || bounds.width <= 0.0 || bounds.height <= 0.0 {
            return None;
        }

        if self.symbol.is_linear() {
            // Only the width is quantised: the bars may be any height, and
            // filling the box with them is what a reader wants.
            let module = (bounds.width / columns).floor().max(1.0);
            let width = module * columns;
            return Some(Placement {
                module,
                origin: Point::new(((bounds.width - width) / 2.0).max(0.0), 0.0),
                row_height: bounds.height,
            });
        }

        let module = (bounds.width / columns)
            .min(bounds.height / rows)
            .floor()
            .max(1.0);
        let (width, height) = (module * columns, module * rows);
        Some(Placement {
            module,
            origin: Point::new(
                ((bounds.width - width) / 2.0).max(0.0),
                ((bounds.height - height) / 2.0).max(0.0),
            ),
            row_height: module,
        })
    }
}

impl<Message> canvas::Program<Message, Theme, Renderer> for Scannable<'_> {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        // The light background is drawn rather than inherited: a barcode over
        // a themed surface is a barcode over whatever colour the desktop feels
        // like today.
        frame.fill(&Path::rectangle(Point::ORIGIN, bounds.size()), Color::WHITE);

        let Some(placement) = self.place(bounds.size()) else {
            return vec![frame.into_geometry()];
        };

        for row in 0..self.symbol.height() {
            let Some(modules) = self.symbol.row(row) else {
                continue;
            };
            let y = placement.origin.y + row as f32 * placement.row_height;
            // Consecutive dark modules become one rectangle. A PDF417 symbol
            // is a few hundred modules wide and thirty rows tall, and drawing
            // each one separately is thousands of quads a frame for a picture
            // that never changes.
            for (start, length) in runs(modules) {
                frame.fill(
                    &Path::rectangle(
                        Point::new(placement.origin.x + start as f32 * placement.module, y),
                        Size::new(length as f32 * placement.module, placement.row_height),
                    ),
                    Color::BLACK,
                );
            }
        }

        vec![frame.into_geometry()]
    }
}

/// The runs of dark modules in one row, as `(start, length)` pairs.
fn runs(modules: &[bool]) -> Vec<(usize, usize)> {
    let mut runs = Vec::new();
    let mut start = None;
    for (index, dark) in modules.iter().enumerate() {
        match (dark, start) {
            (true, None) => start = Some(index),
            (false, Some(from)) => {
                runs.push((from, index - from));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(from) = start {
        runs.push((from, modules.len() - from));
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_merge_adjacent_dark_modules_and_stop_at_the_edge() {
        assert_eq!(runs(&[]), vec![]);
        assert_eq!(runs(&[false, false]), vec![]);
        assert_eq!(runs(&[true, true, false, true]), vec![(0, 2), (3, 1)]);
        // A run that reaches the last module still has to be emitted.
        assert_eq!(runs(&[false, true, true]), vec![(1, 2)]);
    }
}
