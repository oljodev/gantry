//! A document to test against.
//!
//! Compiled always rather than behind a test feature, because the crates that need it are not
//! this one: the filesystem connector and the attachment ingest both want a PDF that is really a
//! PDF, and a test fixture nobody can read is worse than forty lines that say what a PDF is.

/// A minimal uncompressed PDF: one content stream per page, one line of Helvetica per line of
/// `pages`, and a correct cross-reference table.
///
/// Built rather than checked in as a binary, so that what a test feeds the extractor is
/// readable — these forty lines are the whole of what a PDF has to be — and so that the two
/// crates either side of this one test against the same document.
pub fn pdf(pages: &[&str]) -> Vec<u8> {
    let font = 3 + pages.len() * 2;
    let kids: Vec<String> = (0..pages.len())
        .map(|i| format!("{} 0 R", 3 + i * 2))
        .collect();
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        format!(
            "<< /Type /Pages /Kids [{}] /Count {} >>",
            kids.join(" "),
            pages.len()
        ),
    ];
    for (i, text) in pages.iter().enumerate() {
        let content = if text.is_empty() {
            String::new()
        } else {
            let lines: Vec<String> = text
                .lines()
                .enumerate()
                .map(|(n, line)| format!("1 0 0 1 72 {} Tm ({line}) Tj", 720 - n as i32 * 14))
                .collect();
            format!("BT /F1 12 Tf\n{}\nET", lines.join("\n"))
        };
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents {} 0 R \
             /Resources << /Font << /F1 {font} 0 R >> >> >>",
            4 + i * 2
        ));
        objects.push(format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len() + 1
        ));
    }
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned());

    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for offset in &offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}
