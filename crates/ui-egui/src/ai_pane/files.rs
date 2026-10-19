//! Attached files, made cheap to read: documents (Word, OpenDocument, RTF, HTML, LaTeX) are
//! opened by the app itself and handed to the agent as Markdown, and PDFs as their text when
//! `pdftotext` is installed. A `.docx` read as is costs the agent many times more tokens than its
//! words; pictures and plain text go as they are.

use std::path::{Path, PathBuf};

/// Formats the app opens and turns into Markdown.
const DOCUMENTS: [&str; 13] = ["docx", "docm", "dotx", "dotm", "doc", "dot", "odt", "rtf", "html", "htm", "tex", "latex", "ltx"];

/// Where converted copies go: a folder of their own in the temporary folder.
pub fn folder() -> PathBuf {
    std::env::temp_dir().join("word-ai-files")
}

fn ext(p: &Path) -> String {
    p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

/// A name for the converted copy that stays the same for the same file.
fn copy_name(p: &Path) -> String {
    let stem = p.file_stem().map_or_else(|| "file".into(), |s| s.to_string_lossy().into_owned());
    let hash = p.to_string_lossy().bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
    format!("{stem}-{:08x}.md", hash & 0xFFFF_FFFF)
}

/// The text of `p` as Markdown, when it is a document the app or `pdftotext` can read.
pub fn markdown(p: &Path) -> Option<String> {
    let e = ext(p);
    if DOCUMENTS.contains(&e.as_str()) {
        let doc = wordcraft_engine::io::open_path(p).ok()?;
        let bytes = wordcraft_engine::io::save_bytes("file.md", &doc).ok()?;
        return String::from_utf8(bytes).ok().filter(|s| !s.trim().is_empty());
    }
    if e == "pdf" {
        let program = super::agent::find_program("pdftotext")?;
        let out = std::process::Command::new(program).arg("-enc").arg("UTF-8").arg(p).arg("-").output().ok()?;
        // A scanned PDF has no text: the agent then reads the file itself.
        return String::from_utf8(out.stdout).ok().filter(|s| out.status.success() && s.split_whitespace().nth(20).is_some());
    }
    None
}

/// The file the agent should read for `p`: a Markdown copy when one can be made, else `p`.
pub fn for_agent(p: &Path) -> (PathBuf, bool) {
    let Some(md) = markdown(p) else { return (p.to_path_buf(), false) };
    let dir = folder();
    let out = dir.join(copy_name(p));
    if std::fs::create_dir_all(&dir).is_ok() && std::fs::write(&out, md).is_ok() { (out, true) } else { (p.to_path_buf(), false) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_word_document_reaches_the_agent_as_markdown() {
        let dir = std::env::temp_dir().join(format!("word-ai-files-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("informe.docx");
        wordcraft_engine::io::save_path(&src, &wordcraft_engine::sample::sample_document()).unwrap();
        let (path, converted) = for_agent(&src);
        assert!(converted && path.extension().is_some_and(|e| e == "md"), "{path:?}");
        let md = std::fs::read_to_string(&path).unwrap();
        assert!(md.contains("# ") && md.contains("studio"), "{md}");
        assert!(md.len() < std::fs::metadata(&src).unwrap().len() as usize, "smaller than the .docx");
        let picture = dir.join("foto.png");
        assert_eq!(for_agent(&picture), (picture.clone(), false), "pictures go as they are");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
