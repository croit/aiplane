// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Capped access to the zip packages Office and OpenDocument files are.
//!
//! A part is never inflated beyond [`MAX_PART_BYTES`], so a zip bomb costs a
//! bounded read instead of the process's memory.

use std::collections::HashMap;
use std::io::{Cursor, Read, Write};

use zip::write::FileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use super::StampError;

/// The most one XML part is read to: a label part is a few KiB, a document
/// body rarely more than a few MiB.
pub(super) const MAX_PART_BYTES: u64 = 8 * 1024 * 1024;

/// A package with more entries than this is not a document.
pub(super) const MAX_ENTRIES: usize = 10_000;

const OLE_MAGIC: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
const ZIP_MAGIC: [u8; 2] = *b"PK";

pub(super) type Package<'a> = ZipArchive<Cursor<&'a [u8]>>;

pub(super) fn is_ole(bytes: &[u8]) -> bool {
    bytes.starts_with(&OLE_MAGIC)
}

pub(super) fn looks_like_zip(bytes: &[u8]) -> bool {
    bytes.starts_with(&ZIP_MAGIC)
}

pub(super) fn open(bytes: &[u8]) -> Option<Package<'_>> {
    let archive = ZipArchive::new(Cursor::new(bytes)).ok()?;
    (archive.len() <= MAX_ENTRIES).then_some(archive)
}

pub(super) fn part_names(package: &Package<'_>) -> Vec<String> {
    package
        .file_names()
        .filter_map(|name| name.ok().map(|name| name.into_owned()))
        .collect()
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum PartError {
    TooLarge,
    Unreadable,
}

/// The part's bytes, `Ok(None)` when the package has no such part, and an
/// error when the part is damaged or inflates beyond `cap` bytes.
pub(super) fn read_part(
    package: &mut Package<'_>,
    name: &str,
    cap: u64,
) -> Result<Option<Vec<u8>>, PartError> {
    let file = match package.by_name(name) {
        Ok(file) => file,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(_) => return Err(PartError::Unreadable),
    };
    let mut out = Vec::new();
    file.take(cap + 1)
        .read_to_end(&mut out)
        .map_err(|_| PartError::Unreadable)?;
    if out.len() as u64 > cap {
        return Err(PartError::TooLarge);
    }
    Ok(Some(out))
}

/// The package with `replace` parts swapped for new content and `add` parts
/// appended. Every other part is copied as its compressed bytes, so it is
/// byte for byte what it was, and the order (an OpenDocument `mimetype` part
/// must stay first) is kept.
pub(super) fn rewrite(
    bytes: &[u8],
    replace: &HashMap<String, Vec<u8>>,
    add: &[(String, Vec<u8>)],
) -> Result<Vec<u8>, StampError> {
    let fail = |reason: String| StampError::Package { reason };
    let mut package = open(bytes).ok_or_else(|| fail("the file is not a readable zip".into()))?;
    let mut out = Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(&mut out);
    let options: FileOptions<()> =
        FileOptions::default().compression_method(CompressionMethod::Deflated);
    for index in 0..package.len() {
        let entry = package
            .by_index_raw(index)
            .map_err(|e| fail(e.to_string()))?;
        let name = entry.name().map_err(|e| fail(e.to_string()))?.into_owned();
        match replace.get(&name) {
            Some(content) => {
                writer
                    .start_file(name, options)
                    .map_err(|e| fail(e.to_string()))?;
                writer.write_all(content).map_err(|e| fail(e.to_string()))?;
            }
            None => writer
                .raw_copy_file(entry)
                .map_err(|e| fail(e.to_string()))?,
        }
    }
    for (name, content) in add {
        writer
            .start_file(name.clone(), options)
            .map_err(|e| fail(e.to_string()))?;
        writer.write_all(content).map_err(|e| fail(e.to_string()))?;
    }
    writer.finish().map_err(|e| fail(e.to_string()))?;
    Ok(out.into_inner())
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    pub fn build(parts: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        let mut writer = ZipWriter::new(&mut out);
        let options: FileOptions<()> =
            FileOptions::default().compression_method(CompressionMethod::Deflated);
        for (name, content) in parts {
            writer.start_file(*name, options).unwrap();
            writer.write_all(content).unwrap();
        }
        writer.finish().unwrap();
        out.into_inner()
    }

    #[test]
    fn a_part_inflating_past_the_cap_is_refused_without_being_read_whole() {
        let bomb = vec![0u8; 3 * 1024 * 1024];
        let bytes = build(&[("big.xml", &bomb)]);
        assert!(bytes.len() < 20_000, "the fixture must compress hard");
        let mut package = open(&bytes).unwrap();
        assert_eq!(
            read_part(&mut package, "big.xml", 1024 * 1024),
            Err(PartError::TooLarge)
        );
        assert_eq!(
            read_part(&mut package, "big.xml", 4 * 1024 * 1024)
                .unwrap()
                .unwrap()
                .len(),
            bomb.len()
        );
    }

    #[test]
    fn a_missing_part_is_none_and_not_an_error() {
        let bytes = build(&[("a.xml", b"<a/>")]);
        let mut package = open(&bytes).unwrap();
        assert_eq!(read_part(&mut package, "b.xml", 100), Ok(None));
    }

    #[test]
    fn rewriting_replaces_and_adds_parts_and_copies_the_rest_unchanged() {
        let bytes = build(&[
            ("mimetype", b"x"),
            ("keep.xml", b"<keep/>"),
            ("old.xml", b"<old/>"),
        ]);
        let replace = HashMap::from([("old.xml".to_string(), b"<new/>".to_vec())]);
        let add = vec![("added.xml".to_string(), b"<added/>".to_vec())];
        let out = rewrite(&bytes, &replace, &add).unwrap();
        let mut package = open(&out).unwrap();
        assert_eq!(
            part_names(&package),
            ["mimetype", "keep.xml", "old.xml", "added.xml"]
        );
        assert_eq!(
            read_part(&mut package, "old.xml", 100),
            Ok(Some(b"<new/>".to_vec()))
        );
        assert_eq!(
            read_part(&mut package, "keep.xml", 100),
            Ok(Some(b"<keep/>".to_vec()))
        );
        assert_eq!(
            read_part(&mut package, "added.xml", 100),
            Ok(Some(b"<added/>".to_vec()))
        );
    }

    #[test]
    fn ole_and_zip_magic_are_told_apart() {
        assert!(is_ole(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1, 0]));
        assert!(!is_ole(b"PK\x03\x04"));
        assert!(looks_like_zip(b"PK\x03\x04"));
    }
}
