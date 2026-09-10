// SPDX-License-Identifier: MPL-2.0

//! Turning a pass's barcode into modules a screen can draw.
//!
//! The result is a grid of squares, not an image: a renderer that knows the
//! display — its size, its scale factor, whether it is a full-screen
//! presenter or a thumbnail in a list — is the only thing that can decide how
//! many pixels a module gets, and a module that is 3.5 pixels wide on one row
//! and 3 on the next is a barcode a reader will refuse. So [`encode`] settles
//! the symbol and the caller settles the pixels.
//!
//! # Why all four symbologies
//!
//! PassKit defines QR, PDF417, Aztec and Code 128, and the choice is the
//! issuer's, not ours. Airlines use PDF417 and Aztec, events and loyalty
//! schemes use QR, older retail passes use Code 128. Rendering only QR
//! silently excludes every boarding pass, which is the one thing a wallet
//! exists for.
//!
//! # Why `messageEncoding` is obeyed rather than assumed
//!
//! A barcode carries bytes. `pass.json` says which encoding turns the
//! issuer's message into those bytes, and it is almost always `iso-8859-1`.
//! Re-encoding a Latin-1 message as UTF-8 still produces a scannable
//! barcode — it just scans to a different string, which a gate reader
//! rejects and a person cannot debug. So the declared encoding is passed
//! through to the encoder, and an encoding this cannot produce is an error
//! rather than a guess.

use crate::model::{Barcode, BarcodeFormat};
use rxing::{EncodeHintValue, EncodeHints, Writer};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the barcode message is empty")]
    EmptyMessage,
    #[error("{0} is not a character encoding this can produce")]
    UnknownEncoding(String),
    #[error("the message cannot be written as {format}: {reason}")]
    Unencodable {
        format: &'static str,
        reason: String,
    },
}

/// A barcode as a grid of square modules, quiet zone included.
///
/// The quiet zone is part of the grid because it is part of the symbol: a
/// reader needs the clear margin as much as it needs the bars, and a renderer
/// that has to remember to add one is a renderer that will forget.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Symbol {
    width: usize,
    height: usize,
    /// Row-major, `true` for a dark module.
    dark: Vec<bool>,
}

impl Symbol {
    #[must_use]
    pub const fn width(&self) -> usize {
        self.width
    }

    /// The symbol's height in modules.
    ///
    /// One for a linear symbology — Code 128 carries no information
    /// vertically, so the renderer picks a bar height that suits the display.
    /// Anything taller is a 2D symbol and must be drawn square.
    #[must_use]
    pub const fn height(&self) -> usize {
        self.height
    }

    /// Whether the symbol carries its data along one axis only, and so may be
    /// stretched vertically.
    #[must_use]
    pub const fn is_linear(&self) -> bool {
        self.height == 1
    }

    /// Whether the module at `(x, y)` is dark. Out of bounds is light.
    #[must_use]
    pub fn is_dark(&self, x: usize, y: usize) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }
        self.dark[y * self.width + x]
    }

    /// One row of modules, or `None` past the bottom.
    #[must_use]
    pub fn row(&self, y: usize) -> Option<&[bool]> {
        if y >= self.height {
            return None;
        }
        Some(&self.dark[y * self.width..(y + 1) * self.width])
    }
}

impl BarcodeFormat {
    /// The name to put in an error a person will read.
    const fn label(self) -> &'static str {
        match self {
            Self::Qr => "QR",
            Self::Pdf417 => "PDF417",
            Self::Aztec => "Aztec",
            Self::Code128 => "Code 128",
        }
    }

    const fn rxing(self) -> rxing::BarcodeFormat {
        match self {
            Self::Qr => rxing::BarcodeFormat::QR_CODE,
            Self::Pdf417 => rxing::BarcodeFormat::PDF_417,
            Self::Aztec => rxing::BarcodeFormat::AZTEC,
            Self::Code128 => rxing::BarcodeFormat::CODE_128,
        }
    }

    /// The quiet zone to ask for, in modules, where the encoder's own default
    /// is not the one the specification asks for.
    ///
    /// Only PDF417 needs saying: the library defaults to thirty modules a
    /// side, which on a laptop screen shrinks the symbol itself to the point
    /// where a reader cannot resolve it. Two is what the specification
    /// requires. The other three defaults are already the specified ones —
    /// four for QR, ten for Code 128, and none for Aztec, which needs none.
    const fn quiet_zone(self) -> Option<u32> {
        match self {
            Self::Pdf417 => Some(2),
            Self::Qr | Self::Aztec | Self::Code128 => None,
        }
    }
}

/// Encodes a pass's barcode into the modules that represent it.
///
/// # Errors
///
/// Returns [`Error`] when the message is empty, when `messageEncoding` names
/// a character set this cannot produce, or when the message cannot be
/// expressed in the symbology the issuer chose — a Code 128 barcode carrying
/// a character outside ASCII, say, which is a defect in the pass rather than
/// one here.
pub fn encode(barcode: &Barcode) -> Result<Symbol, Error> {
    if barcode.message.is_empty() {
        return Err(Error::EmptyMessage);
    }

    let charset = character_set(&barcode.message_encoding)?;
    let mut hints = EncodeHints::default().with(EncodeHintValue::CharacterSet(charset.to_owned()));
    if let Some(modules) = barcode.format.quiet_zone() {
        hints = hints.with(EncodeHintValue::Margin(modules.to_string()));
    }

    // Zero for both dimensions asks the encoder for the symbol at its natural
    // size — one pixel per module — which is exactly the grid we want to hand
    // back. Any other value would have the library scale it, and it scales by
    // truncating integer division, which is how uneven modules happen.
    let matrix = rxing::MultiFormatWriter
        .encode_with_hints(&barcode.message, &barcode.format.rxing(), 0, 0, &hints)
        .map_err(|why| Error::Unencodable {
            format: barcode.format.label(),
            reason: why.to_string(),
        })?;

    let width = matrix.getWidth() as usize;
    let height = matrix.getHeight() as usize;
    let mut dark = Vec::with_capacity(width * height);
    for y in 0..height {
        for x in 0..width {
            dark.push(matrix.get(x as u32, y as u32));
        }
    }
    Ok(Symbol {
        width,
        height,
        dark,
    })
}

/// Resolves a `messageEncoding` to the canonical name the encoder knows.
///
/// The aliases spelt out here are the ones IANA registers for Latin-1 and
/// that issuers actually write; the rest of the registry the encoder already
/// knows, case-insensitively. An encoding that resolves to nothing is
/// refused: a barcode built from the wrong bytes scans cleanly to the wrong
/// string, and that failure is discovered at a gate rather than here.
fn character_set(declared: &str) -> Result<&'static str, Error> {
    let trimmed = declared.trim();
    let name = match trimmed.to_ascii_lowercase().as_str() {
        // Omitted altogether means Latin-1, which is the default byte
        // interpretation of every symbology here.
        "" | "latin1" | "latin-1" | "l1" | "iso8859-1" | "iso_8859-1" | "iso_8859-1:1987"
        | "iso-ir-100" | "cp819" | "ibm819" | "csisolatin1" => "iso-8859-1",
        "ascii" => "us-ascii",
        _ => trimmed,
    };

    rxing::common::CharacterSet::get_character_set_by_name(name)
        .map(|set| set.get_charset_name())
        .ok_or_else(|| Error::UnknownEncoding(declared.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn barcode(format: BarcodeFormat, message: &str, encoding: &str) -> Barcode {
        Barcode {
            format,
            message: message.to_owned(),
            message_encoding: encoding.to_owned(),
            alt_text: None,
        }
    }

    /// Draws the symbol the way the presenter does — one square block per
    /// module, dark on light — and reads it back with a scanner.
    ///
    /// This is the only test that proves anything a gate cares about. Module
    /// counts and matrix dimensions can all be right while the barcode scans
    /// to the wrong string.
    fn scan(symbol: &Symbol, format: BarcodeFormat) -> String {
        const SCALE: usize = 6;
        // A linear symbol carries nothing vertically, so it is drawn tall for
        // the same reason the presenter draws it tall: a reader sweeps across
        // it and needs somewhere to sweep.
        let rows = if symbol.is_linear() {
            20
        } else {
            symbol.height()
        };

        let width = symbol.width() * SCALE;
        let height = rows * SCALE;
        let mut luma = vec![255u8; width * height];
        for y in 0..height {
            let module_y = if symbol.is_linear() { 0 } else { y / SCALE };
            for x in 0..width {
                if symbol.is_dark(x / SCALE, module_y) {
                    luma[y * width + x] = 0;
                }
            }
        }

        let found =
            rxing::helpers::detect_in_luma(luma, width as u32, height as u32, Some(format.rxing()))
                .expect("the symbol should scan");
        found.getText().to_owned()
    }

    #[test]
    fn every_symbology_a_pass_can_name_scans_back_to_its_message() {
        // The airline formats are first because they are the ones a
        // QR-only wallet drops on the floor.
        for format in [
            BarcodeFormat::Pdf417,
            BarcodeFormat::Aztec,
            BarcodeFormat::Qr,
            BarcodeFormat::Code128,
        ] {
            let message = "M1PRITIS/DOMINIKOS EABC123 ATHLHR";
            let symbol = encode(&barcode(format, message, "iso-8859-1"))
                .unwrap_or_else(|why| panic!("{} should encode: {why}", format.label()));
            assert_eq!(
                scan(&symbol, format),
                message,
                "{} scanned to the wrong string",
                format.label()
            );
        }
    }

    #[test]
    fn a_latin_1_message_scans_back_as_latin_1_not_as_utf_8() {
        // The failure this guards is silent: encoding "Æ" as UTF-8 produces a
        // perfectly scannable barcode that reads "Ã†".
        let message = "BILLETT Æ Ø Å";
        let symbol = encode(&barcode(BarcodeFormat::Qr, message, "iso-8859-1")).unwrap();
        assert_eq!(scan(&symbol, BarcodeFormat::Qr), message);
    }

    #[test]
    fn an_encoding_that_cannot_be_produced_is_refused_rather_than_guessed() {
        let why = encode(&barcode(BarcodeFormat::Qr, "ABC123", "ebcdic-cp-us")).unwrap_err();
        assert!(matches!(why, Error::UnknownEncoding(_)));
    }

    #[test]
    fn the_iana_aliases_issuers_actually_write_resolve_to_latin_1() {
        for alias in ["ISO-8859-1", "iso8859-1", "latin1", " iso-8859-1 ", ""] {
            assert_eq!(
                character_set(alias).unwrap(),
                "iso-8859-1",
                "{alias} should be Latin-1"
            );
        }
    }

    #[test]
    fn a_message_code_128_cannot_carry_is_an_error_naming_the_symbology() {
        let why = encode(&barcode(BarcodeFormat::Code128, "ΑΘΗΝΑ", "iso-8859-1")).unwrap_err();
        let Error::Unencodable { format, .. } = why else {
            panic!("expected an unencodable message, got {why}");
        };
        assert_eq!(format, "Code 128");
    }

    #[test]
    fn an_empty_message_is_an_error_not_an_empty_symbol() {
        let why = encode(&barcode(BarcodeFormat::Qr, "", "iso-8859-1")).unwrap_err();
        assert!(matches!(why, Error::EmptyMessage));
    }

    #[test]
    fn a_linear_symbology_is_one_module_tall_and_a_matrix_one_is_not() {
        let linear = encode(&barcode(BarcodeFormat::Code128, "ABC123", "iso-8859-1")).unwrap();
        assert!(linear.is_linear(), "Code 128 carries nothing vertically");

        let matrix = encode(&barcode(BarcodeFormat::Qr, "ABC123", "iso-8859-1")).unwrap();
        assert!(!matrix.is_linear());
        assert_eq!(matrix.width(), matrix.height(), "a QR symbol is square");
    }

    #[test]
    fn the_quiet_zone_is_part_of_the_symbol() {
        // Four light modules a side is what the QR specification asks for,
        // and a renderer that had to add them itself would forget.
        let symbol = encode(&barcode(BarcodeFormat::Qr, "ABC123", "iso-8859-1")).unwrap();
        for edge in 0..4 {
            assert!(
                symbol.row(edge).unwrap().iter().all(|dark| !dark),
                "row {edge} should be quiet"
            );
        }
        assert!(
            symbol.row(4).unwrap().iter().any(|dark| *dark),
            "the symbol itself should start after the quiet zone"
        );
    }
}
