//! Each document's conversation, kept between runs: what was said, the agent's session (resumed
//! by the next message, so nothing is sent twice) and an unfinished task to pick up again. One
//! small JSON file per document under `<settings>/ai/`, named by a hash of its path.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::agent::Agent;

/// The most messages kept per document; older ones scroll away.
const MAX_MESSAGES: usize = 200;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Msg {
    User {
        text: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        files: Vec<String>,
    },
    Assistant {
        text: String,
    },
    /// A finished turn's work: how many changes it made and how long it took.
    Work {
        edits: u32,
        secs: u32,
    },
    /// The task stopped before the end (the user stopped it, or the agent failed).
    Stopped {
        by_user: bool,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        detail: String,
    },
    /// A message from the pane itself (no agent installed, no file dialog…).
    Note {
        text: String,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Conversation {
    pub agent: Agent,
    pub session: Option<String>,
    pub messages: Vec<Msg>,
    /// The request of a task that was interrupted, for Continue.
    pub pending: Option<String>,
}

impl Conversation {
    pub fn push(&mut self, m: Msg) {
        self.messages.push(m);
        if self.messages.len() > MAX_MESSAGES {
            let extra = self.messages.len() - MAX_MESSAGES;
            self.messages.drain(..extra);
        }
    }
}

/// FNV-1a: a hash that stays the same across builds, so a document finds its file again.
fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

/// The key of the document at `path`; documents never saved share one.
pub fn key(path: Option<&Path>) -> String {
    match path {
        Some(p) => format!("{:016x}", fnv(&p.to_string_lossy())),
        None => "untitled".into(),
    }
}

fn file(dir: &Path, key: &str) -> PathBuf {
    dir.join(format!("{key}.json"))
}

/// The saved conversation, or a fresh one (missing or unreadable file).
pub fn load(dir: Option<&Path>, key: &str) -> Conversation {
    dir.and_then(|d| std::fs::read_to_string(file(d, key)).ok()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

/// Save it; an empty conversation removes its file.
pub fn save(dir: Option<&Path>, key: &str, c: &Conversation) {
    let Some(dir) = dir else { return };
    let path = file(dir, key);
    if c.messages.is_empty() && c.pending.is_none() {
        let _ = std::fs::remove_file(path);
        return;
    }
    if std::fs::create_dir_all(dir).is_ok()
        && let Ok(s) = serde_json::to_string(c)
    {
        let _ = std::fs::write(path, s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_conversation_comes_back_for_its_document_only() {
        let dir = std::env::temp_dir().join(format!("word-ai-store-{}", std::process::id()));
        let a = key(Some(Path::new("/home/u/plan.docx")));
        let b = key(Some(Path::new("/home/u/other.docx")));
        assert_ne!(a, b);
        assert_eq!(a, key(Some(Path::new("/home/u/plan.docx"))), "stable");
        let mut c = Conversation { agent: Agent::Codex, session: Some("t1".into()), ..Default::default() };
        c.push(Msg::User { text: "Hola".into(), files: Vec::new() });
        c.push(Msg::Stopped { by_user: true, detail: String::new() });
        c.pending = Some("Escribe".into());
        save(Some(&dir), &a, &c);
        assert_eq!(load(Some(&dir), &a), c);
        assert_eq!(load(Some(&dir), &b), Conversation::default());
        save(Some(&dir), &a, &Conversation::default());
        assert!(!file(&dir, &a).exists(), "an emptied conversation leaves no file");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn old_messages_scroll_away() {
        let mut c = Conversation::default();
        for i in 0..MAX_MESSAGES + 5 {
            c.push(Msg::Assistant { text: i.to_string() });
        }
        assert_eq!(c.messages.len(), MAX_MESSAGES);
        assert_eq!(c.messages[0], Msg::Assistant { text: "5".into() });
    }
}
