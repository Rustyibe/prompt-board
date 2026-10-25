use std::io::{IsTerminal, Write};
use std::process::{Command, Stdio};

/// 把文本送入 less(优先 $PAGER). 非 TTY、--no-pager 或 pager 不可用时直接打印.
/// -R 保留 ANSI 颜色, -F 内容不满一屏时直接输出退出, -X 退出后不清屏(留在回滚区)
pub fn page(text: &str, no_pager: bool) {
    if no_pager || !std::io::stdout().is_terminal() {
        let _ = std::io::stdout().write_all(text.as_bytes());
        let _ = std::io::stdout().flush();
        return;
    }
    let (bin, extra_args): (String, Vec<String>) = match std::env::var("PAGER") {
        Ok(v) if !v.trim().is_empty() => {
            let mut it = v.split_whitespace();
            let b = it.next().unwrap().to_string();
            (b, it.map(str::to_string).collect())
        }
        _ => ("less".to_string(), vec!["-R".to_string(), "-F".to_string(), "-X".to_string()]),
    };
    let Ok(mut child) = Command::new(&bin)
        .args(extra_args)
        .stdin(Stdio::piped())
        .spawn()
    else {
        let _ = std::io::stdout().write_all(text.as_bytes());
        return;
    };
    if let Some(mut si) = child.stdin.take() {
        let _ = si.write_all(text.as_bytes());
        let _ = si.flush();
    }
    let _ = child.wait();
}
