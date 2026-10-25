use anyhow::{Context, Result};
use base64::Engine as _;
use colored::Colorize;
use std::io::Write;

const DEFAULT_MAX: usize = 100_000;

/// 是否处于 SSH 会话(远程 shell 里本地剪贴板不可达, 需 OSC52)
pub fn is_ssh() -> bool {
    ["SSH_CONNECTION", "SSH_CLIENT", "SSH_TTY"]
        .iter()
        .any(|k| std::env::var_os(k).is_some())
}

fn max_bytes() -> usize {
    std::env::var("PB_OSC52_MAX")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_MAX)
}

/// 生成 OSC52 序列; tmux 里需要 DCS passthrough 包装才能透传给外层终端
pub fn payload_seq(text: &str, tmux: bool) -> String {
    let b64 = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
    let seq = format!("\x1b]52;c;{b64}\x07");
    if tmux {
        format!("\x1bPtmux;\x1b{seq}\x1b\\")
    } else {
        seq
    }
}

/// 截断到 limit 字节且不切断多字节字符
fn floor_boundary(text: &str, limit: usize) -> usize {
    let mut cut = limit.min(text.len());
    while cut > 0 && !text.is_char_boundary(cut) {
        cut -= 1;
    }
    cut
}

/// 把文本通过 OSC52 写入(外层)本地终端的剪贴板
pub fn copy(text: &str) -> Result<()> {
    let limit = max_bytes();
    let payload = if text.len() > limit {
        eprintln!(
            "{} OSC52 内容超过 {} 字节, 已截断 (PB_OSC52_MAX 可调)",
            "警告:".yellow().bold(),
            limit
        );
        &text[..floor_boundary(text, limit)]
    } else {
        text
    };
    let tmux = std::env::var_os("TMUX").is_some();
    let seq = payload_seq(payload, tmux);

    // 优先直写 /dev/tty, 保证 stdout 被重定向时序列仍能达到终端
    if let Ok(mut f) = std::fs::OpenOptions::new().write(true).open("/dev/tty") {
        f.write_all(seq.as_bytes()).context("写入 /dev/tty 失败")?;
        f.flush()?;
    } else {
        let mut so = std::io::stdout().lock();
        so.write_all(seq.as_bytes()).context("OSC52 输出失败")?;
        so.flush()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequence_is_base64_osc52() {
        let s = payload_seq("hi", false);
        assert_eq!(s, "\x1b]52;c;aGk=\x07");
        // tmux 包装
        let t = payload_seq("hi", true);
        assert!(t.starts_with("\x1bPtmux;\x1b"));
        assert!(t.contains("\x1b]52;c;aGk=\x07"));
        assert!(t.ends_with("\x1b\\"));
    }

    #[test]
    fn truncates_at_limit_boundary_safely() {
        // 多字节字符恰好跨在限制边界不应 panic
        let text = "汉".repeat(60_000); // 180KB, 边界落点 100000 = 33333*3+1 → 应回退到整字符
        let cut = floor_boundary(&text, 100_000);
        assert_eq!(cut, 99_999);
        assert!(text.is_char_boundary(cut));
        let _ = &text[..cut]; // 切片不 panic
    }
}
