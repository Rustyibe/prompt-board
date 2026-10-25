use anyhow::{Result, bail};
use std::io::Write;
use std::process::{Command, Stdio};

/// 复制到系统剪贴板: arboard(X11/Wayland) 优先, 失败再退回 wl-copy/xclip/xsel
pub fn copy_text(text: &str) -> Result<()> {
    if let Ok(mut cb) = arboard::Clipboard::new() {
        if cb.set_text(text.to_owned()).is_ok() {
            return Ok(());
        }
    }
    let fallbacks: [(&str, &[&str]); 3] = [
        ("wl-copy", &[]),
        ("xclip", &["-selection", "clipboard"]),
        ("xsel", &["--clipboard", "--input"]),
    ];
    for (bin, args) in fallbacks {
        if try_cmd(bin, args, text) {
            return Ok(());
        }
    }
    bail!(
        "剪贴板不可用(未检测到 X11/Wayland 会话). 可用 `pb show <id>` 选中复制, 或设置 DISPLAY/WAYLAND_DISPLAY"
    )
}

fn try_cmd(bin: &str, args: &[&str], payload: &str) -> bool {
    if which::which(bin).is_err() {
        return false;
    }
    let Ok(mut child) = Command::new(bin)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    if let Some(mut si) = child.stdin.take() {
        let _ = si.write_all(payload.as_bytes());
        let _ = si.flush();
    }
    drop(child.stdin.take());
    matches!(child.wait(), Ok(status) if status.success())
}
