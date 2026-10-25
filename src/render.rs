use colored::Colorize;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::store::Prompt;

fn pad(s: &str, w: usize) -> String {
    format!("{}{}", s, " ".repeat(w.saturating_sub(UnicodeWidthStr::width(s))))
}

/// 按显示宽度截断(中文占两列), 超长补 …
pub(crate) fn trunc(s: &str, w: usize) -> String {
    if UnicodeWidthStr::width(s) <= w {
        return s.to_string();
    }
    let mut acc = 0;
    let mut out = String::new();
    for c in s.chars() {
        let cw = UnicodeWidthChar::width(c).unwrap_or(0);
        if acc + cw > w - 1 {
            break;
        }
        acc += cw;
        out.push(c);
    }
    out.push('…');
    out
}

pub(crate) fn one_line(s: &str) -> String {
    s.lines()
        .find(|l| !l.trim().is_empty())
        .map(|l| l.trim().to_string())
        .unwrap_or_default()
}

/// ls/search 共用的表格: ID · 正文首行摘要 · 更新时间
pub fn ls_table(items: &[&Prompt]) -> String {
    if items.is_empty() {
        return "暂无提示词, 用 `pb add` 保存第一条\n".to_string();
    }
    let prev_w_cap = 64;

    let mut rows: Vec<(String, String, String)> = Vec::new();
    for p in items {
        rows.push((
            p.id.clone(),
            trunc(&one_line(&p.content), prev_w_cap),
            p.updated.clone(),
        ));
    }
    let mut w = [
        UnicodeWidthStr::width("ID"),
        UnicodeWidthStr::width("内容"),
        UnicodeWidthStr::width("更新时间"),
    ];
    for r in &rows {
        w[0] = w[0].max(r.0.width());
        w[1] = w[1].max(r.1.width());
        w[2] = w[2].max(r.2.width());
    }

    let mut out = String::new();
    out.push_str(&format!(
        "{}  {}  {}\n",
        pad("ID", w[0]).yellow().bold(),
        pad("内容", w[1]).yellow(),
        pad("更新时间", w[2]).yellow(),
    ));
    for r in rows {
        out.push_str(&format!(
            "{}  {}  {}\n",
            pad(&r.0, w[0]).green(),
            pad(&r.1, w[1]).dimmed(),
            pad(&r.2, w[2]).dimmed(),
        ));
    }
    out.push('\n');
    out.push_str(
        &format!(
            "共 {} 条 · 详情 `pb show <id>` · 复制 `pb copy <id>` · id 支持唯一前缀, q 退出分页",
            items.len()
        )
        .dimmed()
        .to_string(),
    );
    out.push('\n');
    out
}

pub fn show_text(p: &Prompt) -> String {
    format!(
        "{}\n{}\n\n{}\n",
        format!("■ [{}]", p.id).cyan().bold(),
        format!("创建 {} · 更新 {}", p.created, p.updated).dimmed(),
        p.content,
    )
}
