// SPDX-License-Identifier: MPL-2.0

//! Reading a `.pkpass`.
//!
//! A `.pkpass` is a zip holding `pass.json`, `manifest.json`, a `signature`,
//! and the images. The manifest maps every other file to its SHA-1 digest;
//! the signature is a detached PKCS#7 over the manifest, made with a
//! certificate Apple issued to the pass's team.
//!
//! # What is checked, and what is not
//!
//! [`read`] verifies the **manifest**: every file the manifest names is
//! present with the digest it claims, and no file in the archive is
//! unaccounted for. That catches a truncated download, a corrupted copy, and
//! content appended to the archive after the fact.
//!
//! It does **not** verify the signature, so it does not yet prove the pass
//! came from the issuer it names. That is a real gap and it is on the roadmap
//! rather than papered over: the WWDR chain is public, so the check is
//! implementable, but a function that returned "valid" without doing it would
//! be worse than its absence.
//!
//! SHA-1 is the digest here because PassKit specifies SHA-1 and the file was
//! written by an issuer that had no choice. It is a checksum against
//! corruption in this crate and is not relied on for authenticity.

use crate::model::{Barcode, Field, Pass, PassKind};
use chrono::{DateTime, FixedOffset};
use serde::Deserialize;
use sha1::{Digest, Sha1};
use std::collections::BTreeMap;
use std::io::{Cursor, Read};

/// Files the manifest does not cover, by PassKit's own rule.
const UNMANIFESTED: [&str; 2] = ["manifest.json", "signature"];

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not a zip archive: {0}")]
    Archive(#[from] zip::result::ZipError),
    #[error("reading {file}: {source}")]
    Read {
        file: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{file} is not valid JSON: {source}")]
    Json {
        file: String,
        #[source]
        source: serde_json::Error,
    },
    #[error(
        "no pass style in pass.json — expected one of boardingPass, coupon, eventTicket, storeCard, generic"
    )]
    NoStyle,
    #[error("{0} is missing")]
    Missing(&'static str),
    #[error("{file} is named in the manifest but not in the archive")]
    ManifestMissingFile { file: String },
    #[error("{file} is in the archive but not in the manifest")]
    ManifestExtraFile { file: String },
    #[error("{file} does not match its manifest digest")]
    ManifestDigestMismatch { file: String },
}

/// Reads and verifies a `.pkpass` from its bytes.
///
/// # Errors
///
/// Returns [`Error`] when the archive will not open, when `pass.json` or
/// `manifest.json` is missing or malformed, or when the manifest does not
/// describe the archive exactly.
pub fn read(bytes: &[u8]) -> Result<Pass, Error> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
    let files = read_all(&mut archive)?;
    verify_manifest(&files)?;

    let raw = files
        .get("pass.json")
        .ok_or(Error::Missing("pass.json"))?
        .as_slice();
    let json: serde_json::Value = serde_json::from_slice(raw).map_err(|source| Error::Json {
        file: "pass.json".to_owned(),
        source,
    })?;
    pass_from_json(&json)
}

/// Extracts the issuer's update-service token, if the pass carries one.
///
/// Separate from [`read`] and from [`Pass`] on purpose. This value
/// authenticates as the pass holder against the issuer's web service, so it
/// is a secret; the only correct destination for it is the Secret Service,
/// which on this desktop is Locket. Keeping it off the model means no
/// ordinary code path — a list, a render, a log line — can carry it by
/// accident.
///
/// # Errors
///
/// Returns [`Error`] when the archive will not open or `pass.json` is
/// missing or malformed.
pub fn authentication_token(bytes: &[u8]) -> Result<Option<String>, Error> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
    let raw = read_one(&mut archive, "pass.json")?.ok_or(Error::Missing("pass.json"))?;
    let json: serde_json::Value = serde_json::from_slice(&raw).map_err(|source| Error::Json {
        file: "pass.json".to_owned(),
        source,
    })?;
    Ok(json
        .get("authenticationToken")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned))
}

fn read_all(
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
) -> Result<BTreeMap<String, Vec<u8>>, Error> {
    let mut files = BTreeMap::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_owned();
        let mut buffer = Vec::with_capacity(usize::try_from(entry.size()).unwrap_or(0));
        entry
            .read_to_end(&mut buffer)
            .map_err(|source| Error::Read {
                file: name.clone(),
                source,
            })?;
        files.insert(name, buffer);
    }
    Ok(files)
}

fn read_one(
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
    name: &str,
) -> Result<Option<Vec<u8>>, Error> {
    let mut entry = match archive.by_name(name) {
        Ok(entry) => entry,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(why) => return Err(why.into()),
    };
    let mut buffer = Vec::new();
    entry
        .read_to_end(&mut buffer)
        .map_err(|source| Error::Read {
            file: name.to_owned(),
            source,
        })?;
    Ok(Some(buffer))
}

/// Checks the manifest against the archive in both directions.
///
/// Both directions matter. A file named in the manifest but absent is a
/// truncated archive; a file present but unnamed is content someone appended
/// to a pass that was signed without it.
fn verify_manifest(files: &BTreeMap<String, Vec<u8>>) -> Result<(), Error> {
    let raw = files
        .get("manifest.json")
        .ok_or(Error::Missing("manifest.json"))?;
    let manifest: BTreeMap<String, String> =
        serde_json::from_slice(raw).map_err(|source| Error::Json {
            file: "manifest.json".to_owned(),
            source,
        })?;

    for (name, expected) in &manifest {
        let content = files
            .get(name)
            .ok_or_else(|| Error::ManifestMissingFile { file: name.clone() })?;
        if !digest_matches(content, expected) {
            return Err(Error::ManifestDigestMismatch { file: name.clone() });
        }
    }

    for name in files.keys() {
        if !UNMANIFESTED.contains(&name.as_str()) && !manifest.contains_key(name) {
            return Err(Error::ManifestExtraFile { file: name.clone() });
        }
    }
    Ok(())
}

/// The SHA-1 of `content` as lowercase hex, in the form a manifest holds it.
fn digest_hex(content: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(content);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Issuers write the digest in both cases, so the comparison ignores it.
fn digest_matches(content: &[u8], expected_hex: &str) -> bool {
    digest_hex(content).eq_ignore_ascii_case(expected_hex)
}

/// One `pass.json` field, before its polymorphic `value` is flattened.
#[derive(Deserialize)]
struct RawField {
    key: String,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    value: serde_json::Value,
}

fn pass_from_json(json: &serde_json::Value) -> Result<Pass, Error> {
    let (kind, style) = PassKind::all()
        .into_iter()
        .find_map(|kind| json.get(kind.key()).map(|style| (kind, style)))
        .ok_or(Error::NoStyle)?;

    Ok(Pass {
        kind,
        serial_number: string(json, "serialNumber").unwrap_or_default(),
        pass_type_identifier: string(json, "passTypeIdentifier").unwrap_or_default(),
        team_identifier: string(json, "teamIdentifier").unwrap_or_default(),
        organization_name: string(json, "organizationName").unwrap_or_default(),
        description: string(json, "description").unwrap_or_default(),
        logo_text: string(json, "logoText"),
        transit_type: style
            .get("transitType")
            .and_then(|value| serde_json::from_value(value.clone()).ok()),
        relevant_date: date(json, "relevantDate"),
        expiration_date: date(json, "expirationDate"),
        voided: json
            .get("voided")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        web_service_url: string(json, "webServiceURL"),
        header_fields: fields(style, "headerFields"),
        primary_fields: fields(style, "primaryFields"),
        secondary_fields: fields(style, "secondaryFields"),
        auxiliary_fields: fields(style, "auxiliaryFields"),
        back_fields: fields(style, "backFields"),
        barcodes: barcodes(json),
        background_color: string(json, "backgroundColor"),
        foreground_color: string(json, "foregroundColor"),
        label_color: string(json, "labelColor"),
    })
}

/// `barcodes` is the current key; `barcode` is the pre-iOS-9 singular one.
///
/// Issuers still emit both, the singular for old readers and the plural for
/// new. Taking the plural when it exists and falling back is what PassKit
/// itself does — reading only `barcode` loses the better symbology, and
/// reading only `barcodes` loses old passes entirely.
fn barcodes(json: &serde_json::Value) -> Vec<Barcode> {
    if let Some(list) = json.get("barcodes").and_then(serde_json::Value::as_array) {
        let parsed: Vec<Barcode> = list
            .iter()
            .filter_map(|value| serde_json::from_value(value.clone()).ok())
            .collect();
        if !parsed.is_empty() {
            return parsed;
        }
    }
    json.get("barcode")
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .map(|barcode| vec![barcode])
        .unwrap_or_default()
}

fn fields(style: &serde_json::Value, key: &str) -> Vec<Field> {
    style
        .get(key)
        .and_then(serde_json::Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|value| serde_json::from_value::<RawField>(value.clone()).ok())
                .map(|raw| Field {
                    key: raw.key,
                    label: raw.label,
                    value: match raw.value {
                        serde_json::Value::String(text) => text,
                        serde_json::Value::Null => String::new(),
                        other => other.to_string(),
                    },
                })
                .collect()
        })
        .unwrap_or_default()
}

fn string(json: &serde_json::Value, key: &str) -> Option<String> {
    json.get(key)
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
}

fn date(json: &serde_json::Value, key: &str) -> Option<DateTime<FixedOffset>> {
    string(json, key).and_then(|text| DateTime::parse_from_rfc3339(&text).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TransitType;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    /// Builds a `.pkpass` in memory with a correct manifest, so the tests
    /// exercise the real verification path rather than skipping it.
    fn pkpass(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut manifest = serde_json::Map::new();
        for (name, content) in files {
            manifest.insert(
                (*name).to_owned(),
                serde_json::Value::String(digest_hex(content)),
            );
        }
        let manifest = serde_json::to_vec(&manifest).unwrap();

        let mut buffer = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(Cursor::new(&mut buffer));
            let options = SimpleFileOptions::default();
            for (name, content) in files {
                zip.start_file(*name, options).unwrap();
                zip.write_all(content).unwrap();
            }
            zip.start_file("manifest.json", options).unwrap();
            zip.write_all(&manifest).unwrap();
            zip.finish().unwrap();
        }
        buffer
    }

    const BOARDING: &str = r#"{
        "formatVersion": 1,
        "passTypeIdentifier": "pass.com.example.air",
        "serialNumber": "ABC123",
        "teamIdentifier": "TEAM1",
        "organizationName": "Example Air",
        "description": "Boarding pass ATH to LHR",
        "logoText": "Example Air",
        "relevantDate": "2026-09-20T06:40:00+03:00",
        "expirationDate": "2026-09-20T12:00:00+03:00",
        "authenticationToken": "s3cret-token",
        "webServiceURL": "https://example.test/passes",
        "backgroundColor": "rgb(12,34,56)",
        "barcodes": [
            {
                "format": "PKBarcodeFormatPDF417",
                "message": "M1PRITIS/DOMINIKOS  EABC123 ATHLHRXA",
                "messageEncoding": "iso-8859-1",
                "altText": "ABC123"
            }
        ],
        "boardingPass": {
            "transitType": "PKTransitTypeAir",
            "primaryFields": [
                { "key": "origin", "label": "Athens", "value": "ATH" },
                { "key": "destination", "label": "London", "value": "LHR" }
            ],
            "auxiliaryFields": [
                { "key": "seat", "label": "Seat", "value": 14 }
            ]
        }
    }"#;

    #[test]
    fn a_boarding_pass_round_trips_into_the_model() {
        let bytes = pkpass(&[("pass.json", BOARDING.as_bytes())]);
        let pass = read(&bytes).unwrap();

        assert_eq!(pass.kind, PassKind::BoardingPass);
        assert_eq!(pass.transit_type, Some(TransitType::Air));
        assert_eq!(pass.serial_number, "ABC123");
        assert_eq!(pass.title(), "Example Air");
        assert_eq!(pass.primary_fields.len(), 2);
        assert_eq!(pass.barcode().unwrap().format, crate::BarcodeFormat::Pdf417);
        assert!(pass.relevant_date.is_some());
    }

    #[test]
    fn a_numeric_field_value_survives_as_text() {
        let bytes = pkpass(&[("pass.json", BOARDING.as_bytes())]);
        let pass = read(&bytes).unwrap();
        let seat = &pass.auxiliary_fields[0];
        assert_eq!(seat.key, "seat");
        assert_eq!(seat.value, "14");
    }

    #[test]
    fn the_authentication_token_is_not_on_the_model() {
        let bytes = pkpass(&[("pass.json", BOARDING.as_bytes())]);
        let pass = read(&bytes).unwrap();
        let rendered = serde_json::to_string(&pass).unwrap();
        assert!(
            !rendered.contains("s3cret-token"),
            "the update-service token must never reach the model"
        );
        // It is still reachable, deliberately and only on request.
        assert_eq!(
            authentication_token(&bytes).unwrap().as_deref(),
            Some("s3cret-token")
        );
    }

    #[test]
    fn a_tampered_file_fails_its_manifest_digest() {
        let mut bytes = pkpass(&[("pass.json", BOARDING.as_bytes())]);
        // Rewrite the archive with the same manifest but different content.
        let doctored = BOARDING.replace("Example Air", "Somebody Else");
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes.as_slice())).unwrap();
        let manifest = read_one(&mut archive, "manifest.json").unwrap().unwrap();
        bytes = {
            let mut buffer = Vec::new();
            let mut zip = zip::ZipWriter::new(Cursor::new(&mut buffer));
            let options = SimpleFileOptions::default();
            zip.start_file("pass.json", options).unwrap();
            zip.write_all(doctored.as_bytes()).unwrap();
            zip.start_file("manifest.json", options).unwrap();
            zip.write_all(&manifest).unwrap();
            zip.finish().unwrap();
            buffer
        };

        assert!(matches!(
            read(&bytes),
            Err(Error::ManifestDigestMismatch { .. })
        ));
    }

    #[test]
    fn a_file_appended_after_signing_is_rejected() {
        let mut buffer = Vec::new();
        let signed = pkpass(&[("pass.json", BOARDING.as_bytes())]);
        let mut archive = zip::ZipArchive::new(Cursor::new(signed.as_slice())).unwrap();
        let manifest = read_one(&mut archive, "manifest.json").unwrap().unwrap();
        {
            let mut zip = zip::ZipWriter::new(Cursor::new(&mut buffer));
            let options = SimpleFileOptions::default();
            zip.start_file("pass.json", options).unwrap();
            zip.write_all(BOARDING.as_bytes()).unwrap();
            zip.start_file("manifest.json", options).unwrap();
            zip.write_all(&manifest).unwrap();
            zip.start_file("extra.png", options).unwrap();
            zip.write_all(b"not in the manifest").unwrap();
            zip.finish().unwrap();
        }
        assert!(matches!(
            read(&buffer),
            Err(Error::ManifestExtraFile { .. })
        ));
    }

    #[test]
    fn a_pass_with_no_style_is_an_error_not_a_default() {
        let bytes = pkpass(&[("pass.json", br#"{"serialNumber":"X"}"#)]);
        assert!(matches!(read(&bytes), Err(Error::NoStyle)));
    }

    #[test]
    fn the_legacy_singular_barcode_key_is_still_read() {
        let legacy = r#"{
            "serialNumber": "L1",
            "barcode": {
                "format": "PKBarcodeFormatQR",
                "message": "hello",
                "messageEncoding": "iso-8859-1"
            },
            "storeCard": { "primaryFields": [] }
        }"#;
        let bytes = pkpass(&[("pass.json", legacy.as_bytes())]);
        let pass = read(&bytes).unwrap();
        assert_eq!(pass.kind, PassKind::StoreCard);
        assert_eq!(pass.barcode().unwrap().format, crate::BarcodeFormat::Qr);
    }
}
