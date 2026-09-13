//! An opt-in run over a real document, because the unit tests build their own PDFs and a PDF
//! somebody actually exported is a different animal: compressed streams, subset fonts, custom
//! encodings, tables, two columns.
//!
//! ```sh
//! GANTRY_PDF=~/Downloads/book.pdf cargo test -p gantry-documents --test real -- --nocapture
//! ```
//!
//! It passes when nothing is set, so CI is not waiting on a file nobody has.

#[test]
fn a_real_document_comes_out_as_readable_text() {
    let Ok(path) = std::env::var("GANTRY_PDF") else {
        eprintln!("set GANTRY_PDF to a real PDF to run this");
        return;
    };
    let bytes = std::fs::read(&path).expect("GANTRY_PDF is readable");
    let kind = gantry_documents::kind_of(&path, &bytes).expect("GANTRY_PDF is a document");
    let started = std::time::Instant::now();
    let out = gantry_documents::extract(kind, &path, &bytes).expect("it extracts");
    let lines = out.text.lines().count();
    eprintln!(
        "{} of {} pages, {lines} lines, {} characters in {:?}",
        out.pages_read(),
        out.pages,
        out.text.len(),
        started.elapsed()
    );
    eprintln!("lines 1–40 are pages {:?}", out.pages_for(1, 40));
    for line in out.text.lines().take(12) {
        eprintln!("| {line}");
    }
    assert!(lines > 1, "a real document has more than one line of text");
    assert_eq!(out.pages_for(1, lines).map(|p| p.1), Some(out.pages_read()));
}
