// SPDX-License-Identifier: MPL-2.0

//! The whole path, in the order a traveller walks it: a `.pkpass` lands in
//! the store, the store reads it, and the barcode it carries is scanned.
//!
//! The unit tests each check one joint. This checks that the joints line up —
//! that a pass on disk really does produce a symbol a reader accepts, with
//! the message the issuer wrote. It is as close as a test suite gets to the
//! milestone's own measure, which is a reader at a gate.

use pocket_core::{BarcodeFormat, PassKind, PassStore, barcode};
use sha1::{Digest, Sha1};
use std::io::{Cursor, Write};

/// An Aztec-carrying rail ticket, which is the case a QR-only wallet drops.
const TICKET: &str = r#"{
    "formatVersion": 1,
    "passTypeIdentifier": "pass.com.example.rail",
    "serialNumber": "R-778",
    "teamIdentifier": "TEAM9",
    "organizationName": "Example Rail",
    "description": "Athens to Thessaloniki",
    "logoText": "Example Rail",
    "backgroundColor": "rgb(12, 62, 94)",
    "foregroundColor": "rgb(255, 255, 255)",
    "relevantDate": "2026-10-02T07:15:00+03:00",
    "barcodes": [
        {
            "format": "PKBarcodeFormatAztec",
            "message": "R778 ATH-SKG 02OCT COACH B SEAT 41 PRITIS/D",
            "messageEncoding": "iso-8859-1",
            "altText": "R778"
        }
    ],
    "boardingPass": {
        "transitType": "PKTransitTypeTrain",
        "primaryFields": [
            { "key": "origin", "label": "Athens", "value": "ATH" },
            { "key": "destination", "label": "Thessaloniki", "value": "SKG" }
        ],
        "secondaryFields": [
            { "key": "coach", "label": "Coach", "value": "B" },
            { "key": "seat", "label": "Seat", "value": 41 }
        ]
    }
}"#;

/// Builds a `.pkpass` with a manifest the reader will accept.
fn pkpass(pass_json: &str) -> Vec<u8> {
    let digest: String = Sha1::digest(pass_json.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let manifest = format!(r#"{{"pass.json":"{digest}"}}"#);

    let mut buffer = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(Cursor::new(&mut buffer));
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("pass.json", options).unwrap();
        zip.write_all(pass_json.as_bytes()).unwrap();
        zip.start_file("manifest.json", options).unwrap();
        zip.write_all(manifest.as_bytes()).unwrap();
        zip.finish().unwrap();
    }
    buffer
}

/// Draws the symbol the way the presenter does and reads it back.
fn scan(symbol: &pocket_core::Symbol, format: rxing::BarcodeFormat) -> String {
    const SCALE: usize = 6;
    let rows = if symbol.is_linear() {
        20
    } else {
        symbol.height()
    };
    let (width, height) = (symbol.width() * SCALE, rows * SCALE);

    let mut luma = vec![255u8; width * height];
    for y in 0..height {
        let module_y = if symbol.is_linear() { 0 } else { y / SCALE };
        for x in 0..width {
            if symbol.is_dark(x / SCALE, module_y) {
                luma[y * width + x] = 0;
            }
        }
    }

    rxing::helpers::detect_in_luma(luma, width as u32, height as u32, Some(format))
        .expect("the symbol should scan")
        .getText()
        .to_owned()
}

#[test]
fn a_pass_in_the_store_produces_a_barcode_a_reader_can_read() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(directory.path().join("rail-778")).unwrap();
    std::fs::write(
        directory.path().join("rail-778").join("pass.pkpass"),
        pkpass(TICKET),
    )
    .unwrap();

    let listing = PassStore::open(directory.path()).list().unwrap();
    assert!(
        listing.unreadable.is_empty(),
        "nothing should have failed to read"
    );
    assert_eq!(listing.passes.len(), 1);

    let stored = &listing.passes[0];
    assert_eq!(stored.pass.kind, PassKind::BoardingPass);
    assert_eq!(stored.pass.title(), "Example Rail");
    assert_eq!(
        pocket_core::parse_color(stored.pass.background_color.as_deref().unwrap()),
        Some([12, 62, 94])
    );

    let carried = stored.pass.barcode().expect("the ticket carries a barcode");
    assert_eq!(carried.format, BarcodeFormat::Aztec);

    let symbol = barcode::encode(carried).expect("the barcode should encode");
    assert_eq!(
        scan(&symbol, rxing::BarcodeFormat::AZTEC),
        carried.message,
        "the symbol must scan to exactly what the issuer wrote"
    );
}
