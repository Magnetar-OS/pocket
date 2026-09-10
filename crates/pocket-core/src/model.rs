// SPDX-License-Identifier: MPL-2.0

//! What a pass is, independent of the file it arrived in.
//!
//! The shape follows Apple's PKPass `pass.json` because that is the only
//! interchange format for passes that exists in the wild, not because this
//! crate is an Apple client. A reservation extracted from a PDF or an email
//! lands in the same struct.

use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};

/// The five pass styles. Exactly one appears in a `pass.json`.
///
/// The style is not decoration: it decides the layout, which fields are shown
/// on the face, and — for a boarding pass — that a transit type exists at all.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PassKind {
    BoardingPass,
    Coupon,
    EventTicket,
    StoreCard,
    Generic,
}

impl PassKind {
    /// The `pass.json` key this style's fields live under.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::BoardingPass => "boardingPass",
            Self::Coupon => "coupon",
            Self::EventTicket => "eventTicket",
            Self::StoreCard => "storeCard",
            Self::Generic => "generic",
        }
    }

    /// Every style, in the order a sidebar should list them.
    #[must_use]
    pub const fn all() -> [Self; 5] {
        [
            Self::BoardingPass,
            Self::EventTicket,
            Self::StoreCard,
            Self::Coupon,
            Self::Generic,
        ]
    }
}

/// How a boarding pass travels. Only meaningful on [`PassKind::BoardingPass`].
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum TransitType {
    #[serde(rename = "PKTransitTypeAir")]
    Air,
    #[serde(rename = "PKTransitTypeBoat")]
    Boat,
    #[serde(rename = "PKTransitTypeBus")]
    Bus,
    #[serde(rename = "PKTransitTypeTrain")]
    Train,
    #[serde(rename = "PKTransitTypeGeneric")]
    Generic,
}

/// The barcode symbologies PassKit defines.
///
/// All four matter and none is optional: airlines use PDF417 and Aztec,
/// events and loyalty schemes use QR, and older retail passes use Code 128.
/// A wallet that renders only QR cannot show a boarding pass.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum BarcodeFormat {
    #[serde(rename = "PKBarcodeFormatQR")]
    Qr,
    #[serde(rename = "PKBarcodeFormatPDF417")]
    Pdf417,
    #[serde(rename = "PKBarcodeFormatAztec")]
    Aztec,
    #[serde(rename = "PKBarcodeFormatCode128")]
    Code128,
}

/// One barcode, as the issuer wrote it.
///
/// `message_encoding` is carried verbatim and is almost always
/// `iso-8859-1`. It is not cosmetic: the bytes fed to the encoder have to be
/// the bytes the scanner expects, and re-encoding a Latin-1 message as UTF-8
/// produces a barcode that scans to the wrong string.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Barcode {
    pub format: BarcodeFormat,
    pub message: String,
    pub message_encoding: String,
    #[serde(default)]
    pub alt_text: Option<String>,
}

/// One labelled value on a pass.
///
/// `value` is flattened to a string here. `pass.json` allows a string, a
/// number, or an ISO 8601 date, and the distinction only affects formatting —
/// which is a front-end concern, and one that needs the user's locale rather
/// than the issuer's.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Field {
    pub key: String,
    pub label: Option<String>,
    pub value: String,
}

/// A pass, with everything the face and the back need to be drawn.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Pass {
    pub kind: PassKind,
    /// Unique per `pass_type_identifier`, and the issuer's handle on this
    /// pass — the update web service is addressed by the pair.
    pub serial_number: String,
    pub pass_type_identifier: String,
    pub team_identifier: String,
    pub organization_name: String,
    /// Not shown on the face. It is the accessibility label for the whole
    /// pass, and PassKit requires it.
    pub description: String,
    pub logo_text: Option<String>,
    pub transit_type: Option<TransitType>,
    /// When this pass matters. It is what a wallet sorts on, what decides
    /// which pass is "next", and what a calendar entry would be written from.
    pub relevant_date: Option<DateTime<FixedOffset>>,
    pub expiration_date: Option<DateTime<FixedOffset>>,
    /// The issuer has cancelled it. A voided pass is kept and shown struck
    /// through rather than deleted — it is a record of something that
    /// happened, and deleting it silently is how a user loses a receipt.
    pub voided: bool,
    /// Present when the issuer runs an update service. The token that
    /// authenticates against it is *not* here; see
    /// [`crate::pkpass::authentication_token`].
    pub web_service_url: Option<String>,
    pub header_fields: Vec<Field>,
    pub primary_fields: Vec<Field>,
    pub secondary_fields: Vec<Field>,
    pub auxiliary_fields: Vec<Field>,
    pub back_fields: Vec<Field>,
    pub barcodes: Vec<Barcode>,
    /// CSS-style `rgb(r,g,b)` as the issuer wrote it, carried verbatim.
    pub background_color: Option<String>,
    pub foreground_color: Option<String>,
    pub label_color: Option<String>,
}

impl Pass {
    /// The line to show when the pass is one row in a list.
    #[must_use]
    pub fn title(&self) -> &str {
        self.logo_text
            .as_deref()
            .filter(|text| !text.is_empty())
            .unwrap_or(&self.organization_name)
    }

    /// The barcode to present, preferring the first the issuer listed.
    ///
    /// `barcodes` is ordered by the issuer's preference, and PassKit's own
    /// rule is to take the first one the reader supports. All four formats
    /// are supported here, so that is simply the first.
    #[must_use]
    pub fn barcode(&self) -> Option<&Barcode> {
        self.barcodes.first()
    }

    /// Whether the pass has expired as of `now`.
    ///
    /// A pass with no expiry never expires — that is PassKit's rule, and it
    /// is why a loyalty card stays in the wallet indefinitely.
    #[must_use]
    pub fn is_expired(&self, now: DateTime<FixedOffset>) -> bool {
        self.expiration_date.is_some_and(|expiry| expiry < now)
    }
}

/// Resolves a `pass.json` colour to its red, green and blue components.
///
/// The model keeps the issuer's string verbatim — the file is the source of
/// truth and nothing here rewrites it — so this is a reader rather than a
/// field. PassKit writes `rgb(90, 60, 3)`; a hex value is accepted too
/// because issuers write those as well and refusing one costs the pass its
/// colours for no benefit.
///
/// Returns `None` for anything else, which the caller should treat as "the
/// issuer expressed no preference" and fall back to the desktop's own theme.
/// Guessing at a malformed colour risks illegible text on a card.
#[must_use]
pub fn parse_color(text: &str) -> Option<[u8; 3]> {
    let text = text.trim();

    if let Some(hex) = text.strip_prefix('#') {
        return match hex.len() {
            // The three-digit form doubles each digit, as CSS does.
            3 => {
                let mut channels = hex
                    .chars()
                    .map(|digit| digit.to_digit(16).map(|value| (value * 17) as u8));
                Some([channels.next()??, channels.next()??, channels.next()??])
            }
            6 => {
                let mut channels = (0..3)
                    .map(|index| u8::from_str_radix(hex.get(index * 2..index * 2 + 2)?, 16).ok());
                Some([channels.next()??, channels.next()??, channels.next()??])
            }
            _ => None,
        };
    }

    let inside = text
        .strip_prefix("rgb(")
        .or_else(|| text.strip_prefix("RGB("))?
        .strip_suffix(')')?;
    let mut channels = inside.split(',').map(|part| part.trim().parse::<u8>().ok());
    let color = [channels.next()??, channels.next()??, channels.next()??];
    // A fourth component means this is `rgba(…)` wearing an `rgb(…)` label, or
    // simply malformed. Either way it is not something to guess at.
    if channels.next().is_some() {
        return None;
    }
    Some(color)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_colour_forms_issuers_write_are_all_read() {
        assert_eq!(parse_color("rgb(90, 60, 3)"), Some([90, 60, 3]));
        assert_eq!(parse_color("rgb(0,0,0)"), Some([0, 0, 0]));
        assert_eq!(parse_color(" rgb(255, 255, 255) "), Some([255, 255, 255]));
        assert_eq!(parse_color("#1a2b3c"), Some([0x1a, 0x2b, 0x3c]));
        assert_eq!(parse_color("#f00"), Some([255, 0, 0]));
    }

    #[test]
    fn a_colour_that_cannot_be_read_is_none_rather_than_black() {
        // Black would be a plausible-looking guess, and a plausible-looking
        // guess is how a pass ends up with black text on a black card.
        for malformed in [
            "",
            "red",
            "rgb(90, 60)",
            "rgb(90, 60, 3, 0.5)",
            "rgb(300, 0, 0)",
            "#12345",
            "rgb(90 60 3)",
        ] {
            assert_eq!(parse_color(malformed), None, "{malformed} should not parse");
        }
    }
}
