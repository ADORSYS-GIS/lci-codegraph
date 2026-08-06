//! Shared helpers for the black-box integration suites under `tests/`. Kept in a `tests/common/`
//! subdirectory (not `tests/common.rs`) so cargo does not treat it as its own test binary — the
//! standard pattern for code shared across integration-test crates.

use std::fs;
use std::path::Path;

/// Write `body` to `root/rel`, creating parent directories as needed. Mirrors the private helper
/// `src/walk.rs`'s own `#[cfg(test)]` module uses, duplicated here because integration tests can't
/// reach into a crate's private test-only items.
#[allow(dead_code)]
pub fn write(root: &Path, rel: &str, body: &str) {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, body).unwrap();
}

/// Write raw `bytes` to `root/rel`, creating parent directories as needed — for binary/PDF fixtures.
#[allow(dead_code)]
pub fn write_bytes(root: &Path, rel: &str, bytes: &[u8]) {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, bytes).unwrap();
}

/// A genuinely valid, minimal single-page PDF whose content stream draws `text` — built with `lopdf`
/// (a proper xref/trailer is fiddly to hand-write) so extraction through `pdf-extract` is a real
/// round trip, not a fake fixture. Lifted from the idiom `src/pdf.rs`'s own unit tests use
/// (`minimal_pdf_with_text`), duplicated here because it's a private test-only helper there too.
#[allow(dead_code)]
pub fn minimal_pdf_with_text(text: &str) -> Vec<u8> {
    use lopdf::content::{Content, Operation};
    use lopdf::{Document, Object, Stream, dictionary};

    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let resources_id = doc.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
    });
    let content = Content {
        operations: vec![
            Operation::new("BT", vec![]),
            Operation::new("Tf", vec!["F1".into(), 24.into()]),
            Operation::new("Td", vec![20.into(), 100.into()]),
            Operation::new("Tj", vec![Object::string_literal(text)]),
            Operation::new("ET", vec![]),
        ],
    };
    let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
        "Resources" => resources_id,
        "MediaBox" => vec![0.into(), 0.into(), 200.into(), 200.into()],
    });
    let pages = dictionary! {
        "Type" => "Pages",
        "Kids" => vec![page_id.into()],
        "Count" => 1,
    };
    doc.objects.insert(pages_id, Object::Dictionary(pages));
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);

    let mut buf = Vec::new();
    doc.save_to(&mut buf).expect("save synthetic pdf");
    buf
}
