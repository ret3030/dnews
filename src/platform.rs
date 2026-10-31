//! Small OS integrations: opening a link in the browser and copying text.

use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{Result, bail};
use crossterm::clipboard::CopyToClipboard;

/// Opens `url` in the default browser. Only http(s) URLs are accepted, so a
/// feed can't smuggle a local path or a flag (`-a Calculator`) into `open`.
pub fn open_url(url: &str) -> Result<()> {
    if !is_web_url(url) {
        bail!("not an http(s) link");
    }
    let mut cmd = if cfg!(target_os = "macos") {
        let mut c = Command::new("open");
        c.arg(url);
        c
    } else if cfg!(target_os = "windows") {
        let mut c = Command::new("cmd");
        c.args(["/C", "start", "", url]);
        c
    } else {
        let mut c = Command::new("xdg-open");
        c.arg(url);
        c
    };
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(())
}

/// Copies `text` via the OSC 52 escape sequence, which the terminal itself
/// handles. Works over SSH and needs no clipboard tool installed; supported
/// by kitty, iTerm2, WezTerm, Alacritty, Windows Terminal and tmux.
pub fn copy_to_clipboard(text: &str) -> Result<()> {
    let mut out = std::io::stdout();
    crossterm::execute!(out, CopyToClipboard::to_clipboard_from(text))?;
    out.flush()?;
    Ok(())
}

pub fn is_web_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.starts_with("https://") || lower.starts_with("http://")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_web_urls_are_opened() {
        assert!(is_web_url("https://example.com/a"));
        assert!(is_web_url("HTTP://example.com"));
        assert!(!is_web_url("-a Calculator"));
        assert!(!is_web_url("file:///etc/passwd"));
        assert!(!is_web_url("javascript:alert(1)"));
        assert!(open_url("file:///etc/passwd").is_err());
    }
}
