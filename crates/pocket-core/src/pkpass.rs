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

/// The most entries an archive may hold.
///
/// A pass is `pass.json`, the manifest, the signature, a dozen images at
/// three scales, and optionally a `.lproj` folder of strings and images per
/// language. A thousand covers every language PassKit localises into, many
/// times over.
pub(crate) const MAX_ENTRIES: usize = 1024;

/// The most one entry may hold once decompressed: 16 MiB.
///
/// The largest thing in a real pass is a `@3x` background or strip image, a
/// few hundred kilobytes to a megabyte or two.
pub(crate) const MAX_ENTRY_BYTES: u64 = 16 * 1024 * 1024;

/// The most a whole archive may hold, compressed or decompressed: 64 MiB.
///
/// Real passes are kilobytes to a few megabytes. The limit exists so that a
/// hostile or broken archive — a deflate bomb, or a header claiming an
/// exabyte — is refused as one unreadable pass instead of exhausting memory
/// and taking every other pass in the wallet down with the process.
pub(crate) const MAX_ARCHIVE_BYTES: u64 = 64 * 1024 * 1024;

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
    let files = read_all(bytes)?;
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
    // The same reader as `read`, so the same limits hold.
    let files = read_all(bytes)?;
    let raw = files.get("pass.json").ok_or(Error::Missing("pass.json"))?;
    let json: serde_json::Value = serde_json::from_slice(raw).map_err(|source| Error::Json {
        file: "pass.json".to_owned(),
        source,
    })?;
    Ok(json
        .get("authenticationToken")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned))
}

/// A limit an archive broke, as the [`Error::Read`] it is reported as.
fn refused(file: &str, why: String) -> Error {
    Error::Read {
        file: file.to_owned(),
        source: std::io::Error::new(std::io::ErrorKind::FileTooLarge, why),
    }
}

/// Every file in the archive, by name, within the limits above.
///
/// Nothing the archive *says* about itself is trusted for sizing: the
/// declared uncompressed size decides only whether an entry is refused
/// outright, never how much is allocated, and every read is cut off one byte
/// past what is left of the budget, so an entry that lies about its size is
/// caught by what it actually produces.
fn read_all(bytes: &[u8]) -> Result<BTreeMap<String, Vec<u8>>, Error> {
    if bytes.len() as u64 > MAX_ARCHIVE_BYTES {
        return Err(refused(
            "the archive",
            format!("it is larger than {} MiB", MAX_ARCHIVE_BYTES >> 20),
        ));
    }
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
    if archive.len() > MAX_ENTRIES {
        return Err(refused(
            "the archive",
            format!("it holds {} files, more than {MAX_ENTRIES}", archive.len()),
        ));
    }

    let mut files = BTreeMap::new();
    let mut budget = MAX_ARCHIVE_BYTES;
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_owned();
        let limit = MAX_ENTRY_BYTES.min(budget);
        if entry.size() > limit {
            return Err(too_large(&name, limit));
        }
        let mut buffer = Vec::new();
        entry
            .take(limit + 1)
            .read_to_end(&mut buffer)
            .map_err(|source| Error::Read {
                file: name.clone(),
                source,
            })?;
        let read = buffer.len() as u64;
        if read > limit {
            return Err(too_large(&name, limit));
        }
        budget -= read;
        files.insert(name, buffer);
    }
    Ok(files)
}

/// The error for an entry past `limit`: the per-file limit, or what is left
/// of the archive's.
fn too_large(name: &str, limit: u64) -> Error {
    if limit == MAX_ENTRY_BYTES {
        refused(
            name,
            format!("it is larger than {} MiB", MAX_ENTRY_BYTES >> 20),
        )
    } else {
        refused(
            name,
            format!(
                "the archive decompresses to more than {} MiB",
                MAX_ARCHIVE_BYTES >> 20
            ),
        )
    }
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

/// A `pass.json` date: a W3C timestamp, "a complete date plus hours and
/// minutes" or "… plus hours, minutes and seconds" (Apple's Wallet Developer
/// Guide). RFC 3339 covers the second form only, and the first is the one
/// Apple's own examples use (`2014-12-05T09:00-08:00`).
fn date(json: &serde_json::Value, key: &str) -> Option<DateTime<FixedOffset>> {
    let text = string(json, key)?;
    DateTime::parse_from_rfc3339(&text).ok().or_else(|| {
        // Without seconds. `%:z` takes `±hh:mm` but not the `Z` designator,
        // which means the same as `+00:00`.
        let offset = text
            .strip_suffix('Z')
            .map_or_else(|| text.clone(), |bare| format!("{bare}+00:00"));
        DateTime::parse_from_str(&offset, "%Y-%m-%dT%H:%M%:z").ok()
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::model::TransitType;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    /// Builds a `.pkpass` in memory with a correct manifest, so the tests
    /// exercise the real verification path rather than skipping it.
    pub(crate) fn pkpass(files: &[(&str, &[u8])]) -> Vec<u8> {
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

    pub(crate) const BOARDING: &str = r#"{
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
        let manifest = read_all(&bytes).unwrap().remove("manifest.json").unwrap();
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
        let manifest = read_all(&signed).unwrap().remove("manifest.json").unwrap();
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

    /// PassKit's dates are W3C timestamps, and "a complete date plus hours
    /// and minutes" — no seconds — is the form Apple's own examples use. A
    /// date read as absent sorts a boarding pass below the loyalty cards.
    #[test]
    fn a_date_without_seconds_is_still_a_date() {
        let pass_json = BOARDING
            .replace("2026-09-20T06:40:00+03:00", "2026-09-20T06:40+03:00")
            .replace("2026-09-20T12:00:00+03:00", "2026-09-20T12:00Z");
        let bytes = pkpass(&[("pass.json", pass_json.as_bytes())]);
        let pass = read(&bytes).unwrap();

        let relevant = pass.relevant_date.expect("the relevant date is read");
        assert_eq!(relevant.to_rfc3339(), "2026-09-20T06:40:00+03:00");
        let expiry = pass.expiration_date.expect("the expiry is read");
        assert_eq!(expiry.to_rfc3339(), "2026-09-20T12:00:00+00:00");
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

    /// A zip with one stored entry whose zip64 header claims `claimed`
    /// bytes, whatever it really holds. Written by hand because no zip writer
    /// will produce a header that lies.
    pub(crate) fn lying_archive(name: &str, content: &[u8], claimed: u64) -> Vec<u8> {
        fn crc32(bytes: &[u8]) -> u32 {
            let mut crc = !0u32;
            for byte in bytes {
                crc ^= u32::from(*byte);
                for _ in 0..8 {
                    crc = if crc & 1 == 1 {
                        (crc >> 1) ^ 0xEDB8_8320
                    } else {
                        crc >> 1
                    };
                }
            }
            !crc
        }
        let name_len = u16::try_from(name.len()).unwrap();
        let size = content.len() as u64;
        // The zip64 extra field: uncompressed size, then compressed size.
        let mut extra = Vec::new();
        extra.extend_from_slice(&1u16.to_le_bytes());
        extra.extend_from_slice(&16u16.to_le_bytes());
        extra.extend_from_slice(&claimed.to_le_bytes());
        extra.extend_from_slice(&size.to_le_bytes());

        let mut out = Vec::new();
        // Local file header.
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&45u16.to_le_bytes()); // version needed: zip64
        out.extend_from_slice(&[0; 6]); // flags, method (stored), time
        out.extend_from_slice(&0u16.to_le_bytes()); // date
        out.extend_from_slice(&crc32(content).to_le_bytes());
        out.extend_from_slice(&u32::MAX.to_le_bytes()); // compressed: see zip64
        out.extend_from_slice(&u32::MAX.to_le_bytes()); // uncompressed: see zip64
        out.extend_from_slice(&name_len.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&extra);
        out.extend_from_slice(content);

        let central = out.len();
        out.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        out.extend_from_slice(&45u16.to_le_bytes()); // made by
        out.extend_from_slice(&45u16.to_le_bytes()); // needed
        out.extend_from_slice(&[0; 8]); // flags, method, time, date
        out.extend_from_slice(&crc32(content).to_le_bytes());
        out.extend_from_slice(&u32::MAX.to_le_bytes());
        out.extend_from_slice(&u32::MAX.to_le_bytes());
        out.extend_from_slice(&name_len.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes()); // extra length
        out.extend_from_slice(&[0; 10]); // comment, disk, attributes
        out.extend_from_slice(&0u32.to_le_bytes()); // local header offset
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&extra);
        let central_len = out.len() - central;

        // End of central directory.
        out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        out.extend_from_slice(&[0; 4]); // disk numbers
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&u32::try_from(central_len).unwrap().to_le_bytes());
        out.extend_from_slice(&u32::try_from(central).unwrap().to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    fn is_refused(result: &Result<Pass, Error>) -> bool {
        matches!(
            result,
            Err(Error::Read { source, .. }) if source.kind() == std::io::ErrorKind::FileTooLarge
        )
    }

    /// A header claiming a terabyte is refused on the claim. Sizing a buffer
    /// from it aborted the process ("memory allocation of 1099511627776 bytes
    /// failed"), and the whole wallet with it.
    #[test]
    fn an_entry_claiming_an_enormous_size_is_refused_not_allocated() {
        let bytes = lying_archive("pass.json", b"{}", 1 << 40);
        assert!(is_refused(&read(&bytes)), "{:?}", read(&bytes));
    }

    /// A deflate bomb: kilobytes on disk, more than a pass may hold once
    /// inflated. Cut off at the limit rather than inflated in full.
    #[test]
    fn an_entry_that_inflates_past_the_limit_is_refused() {
        let zeros = vec![0u8; usize::try_from(MAX_ENTRY_BYTES).unwrap() + 1];
        let bytes = pkpass(&[("pass.json", BOARDING.as_bytes()), ("strip.png", &zeros)]);
        assert!(
            bytes.len() < 1024 * 1024,
            "the bomb should be small on disk"
        );
        assert!(is_refused(&read(&bytes)));
    }

    /// Files each under the per-file limit that together inflate past the
    /// archive's.
    #[test]
    fn an_archive_that_inflates_past_its_total_is_refused() {
        let chunk = vec![0u8; usize::try_from(MAX_ENTRY_BYTES).unwrap()];
        let names = ["a.png", "b.png", "c.png", "d.png", "e.png"];
        let mut files: Vec<(&str, &[u8])> = vec![("pass.json", BOARDING.as_bytes())];
        files.extend(names.iter().map(|name| (*name, chunk.as_slice())));
        assert!(is_refused(&read(&pkpass(&files))));
    }

    #[test]
    fn an_archive_with_too_many_files_is_refused() {
        let names: Vec<String> = (0..=MAX_ENTRIES).map(|i| format!("{i}.png")).collect();
        let mut files: Vec<(&str, &[u8])> = vec![("pass.json", BOARDING.as_bytes())];
        files.extend(names.iter().map(|name| (name.as_str(), b"x".as_slice())));
        assert!(is_refused(&read(&pkpass(&files))));
    }

    /// An archive that names `pass.json` twice. The zip reader keeps one
    /// entry per name, and `read` and `authentication_token` must agree on
    /// which: otherwise the token handed to the Secret Service could belong
    /// to a different pass from the one on screen.
    #[test]
    fn a_duplicated_pass_json_is_read_the_same_way_by_both_readers() {
        let decoy = BOARDING
            .replace("Example Air", "Somebody Else")
            .replace("s3cret-token", "decoy-token");
        let manifest = format!(r#"{{"pass.json":"{}"}}"#, digest_hex(decoy.as_bytes()));
        let mut bytes = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(Cursor::new(&mut bytes));
            let options = SimpleFileOptions::default();
            for (name, content) in [
                ("pass.json", BOARDING.as_bytes()),
                ("pass.jsoo", decoy.as_bytes()),
                ("manifest.json", manifest.as_bytes()),
            ] {
                zip.start_file(name, options).unwrap();
                zip.write_all(content).unwrap();
            }
            zip.finish().unwrap();
        }
        // No zip writer will write one name twice, so the second is renamed
        // in place, in its local and its central header: same length, and the
        // checksums cover the data, not the name.
        let (from, to) = (b"pass.jsoo".as_slice(), b"pass.json".as_slice());
        let mut at = 0;
        while let Some(found) = bytes[at..].windows(from.len()).position(|w| w == from) {
            bytes[at + found..at + found + from.len()].copy_from_slice(to);
            at += found + from.len();
        }

        let token = authentication_token(&bytes).unwrap();
        match read(&bytes) {
            Ok(pass) => assert_eq!(
                pass.title() == "Somebody Else",
                token.as_deref() == Some("decoy-token"),
                "the pass and its token came from different copies of pass.json"
            ),
            // The copy the manifest does not vouch for was chosen: refused.
            Err(why) => assert!(matches!(why, Error::ManifestDigestMismatch { .. })),
        }
    }
}
