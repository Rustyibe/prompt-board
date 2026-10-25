mod clip;
mod osc52;
mod pager;
mod render;
mod store;

use anyhow::{Context, Result, bail};
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::engine::{ArgValueCompleter, CompletionCandidate};
use colored::Colorize;
use std::ffi::OsStr;
use std::io::{IsTerminal, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use store::Store;

const AFTER_HELP: &str = r#"
关于 id:
  · 每条 AI 提示词固定一个 6 位 id(去掉易混淆的 0 o 1 l i), 双击即可整段选中
  · show/copy/edit/rm 支持唯一前缀: `pb copy 4d7` 即 `4d7kqm`
  · `pb copy <id>` 复制正文; `pb copy <id> --id` 复制 id 本身(方便分享)
  · tab 时动态列出全部 id(附正文首行摘要): `pb copy <TAB>`

示例:
  pb add                                # 打开 $EDITOR, 粘贴内容后保存
  pb add -c "请把以下内容翻译成地道英文:…"
  pb add < saved.txt                    # 从 stdin 读入
  pb add -f saved.txt                   # 从文件读入
  pb ls                                 # 浏览列表(less, q 退出)
  pb show 4d7                           # 查看全文
  pb copy 4d7                           # 复制到剪贴板
  pb edit 4d7                           # $EDITOR 修改内容
  pb rm 4d7                             # 删除
  pb prune                              # 一键清空(交互确认; --yes 跳过)
  pb search 翻译                        # 多词为 AND: pb search a b
  pb ls --no-pager                      # 不分页直接输出

Tab 补全(一次性安装, 生成的脚本含动态 id 补全):
  fish:  pb completions fish > ~/.config/fish/completions/pb.fish
  zsh:   pb completions zsh  > ~/.zsh/completions/_pb   # 需 fpath 含该目录
  bash:  pb completions bash > ~/.local/share/bash-completion/completions/pb

SSH 远程复制(OSC52):
  · 在远程服务器上 `pb copy <id>` 会自动检测 SSH 会话, 通过 OSC52 把内容送回你本地终端的剪贴板
  · 需要本地终端支持 OSC52: iTerm2 / kitty / WezTerm / Windows Terminal / PuTTY(需勾选)等
  · tmux 里自动加 DCS passthrough; 若被吞, tmux 配置 `set -g allow-passthrough on`
  · 手动控制: `--osc` 强制 OSC52, `--local` 强制系统剪贴板; 上限 100KB (PB_OSC52_MAX)

数据文件: ~/.local/share/prompt-board/prompts.json  (环境变量 PB_DATA_FILE 可覆盖)
编辑器:   add/edit 使用 PB_EDITOR > $VISUAL > $EDITOR > vi   (如 set -Ux PB_EDITOR hx)
分页器:   默认 less -R, 可用 $PAGER 覆盖, --no-pager 关闭
"#;

#[derive(Parser)]
#[command(
    name = "pb",
    version,
    about = "pb — AI 提示词板: 存取常用 prompt, less 浏览, 一键复制",
    after_help = AFTER_HELP
)]
struct Cli {
    /// 不使用 less 分页, 直接输出
    #[arg(long, global = true)]
    no_pager: bool,
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 保存一条 AI 提示词(默认打开 $EDITOR 粘贴/编辑)
    Add {
        /// 直接给出内容
        #[arg(short = 'c', long, value_name = "TEXT")]
        content: Option<String>,
        /// 从文件读取内容(- 表示 stdin)
        #[arg(short = 'f', long, value_name = "PATH")]
        file: Option<PathBuf>,
    },
    /// 列出全部提示词(送入 less)
    #[command(alias = "list")]
    Ls,
    /// 查看一条提示词全文(直接输出)
    Show {
        #[arg(value_name = "ID")]
        id: String,
    },
    /// 复制内容到剪贴板(SSH 会话中自动改走 OSC52 送回本地终端)
    Copy {
        #[arg(value_name = "ID")]
        id: String,
        /// 复制 id 本身而非正文
        #[arg(long = "id")]
        id_only: bool,
        /// 强制走 OSC52(本地终端也用它)
        #[arg(long = "osc")]
        osc: bool,
        /// 强制走系统剪贴板, 不走 OSC52
        #[arg(long)]
        local: bool,
    },
    /// 用 $EDITOR 编辑正文
    Edit {
        #[arg(value_name = "ID")]
        id: String,
    },
    /// 删除
    Rm {
        #[arg(value_name = "ID")]
        id: String,
        /// 跳过确认
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// 清空全部提示词
    Prune {
        /// 跳过确认
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// 关键字搜索(正文, 多词 AND)
    Search {
        #[arg(value_name = "关键词")]
        keywords: Vec<String>,
    },
    /// 输出 shell tab 补全脚本(含动态 id 补全)到 stdout
    Completions { shell: clap_complete::Shell },
}

/// tab 时生成候选: 全部 id(附标题说明), 按已输入前缀过滤
fn ids_candidates(prompts: &[store::Prompt], current: &OsStr) -> Vec<CompletionCandidate> {
    let cur = current.to_string_lossy();
    prompts
        .iter()
        .filter(|p| p.id.starts_with(cur.as_ref()))
        .map(|p| {
            let preview = render::trunc(&render::one_line(&p.content), 40);
            CompletionCandidate::new(p.id.clone()).help(Some(preview.into()))
        })
        .collect()
}

fn complete_ids(current: &OsStr) -> Vec<CompletionCandidate> {
    let Ok(store) = Store::load() else {
        return Vec::new();
    };
    ids_candidates(store.list(), current)
}

/// 挂上动态补全器的 Command: 给各子命令的 <ID> 参数注入 ArgValueCompleter
fn pb_command() -> clap::Command {
    Cli::command().mut_subcommands(|sub| {
        if sub.get_arguments().any(|a| a.get_id().as_str() == "id") {
            sub.mut_arg("id", |arg| arg.add(ArgValueCompleter::new(complete_ids)))
        } else {
            sub
        }
    })
}

fn main() -> ExitCode {
    // shell 补全回调(COMPLETE 协议)最先处理: 动态 id 候选走这里, 命中则直接退出
    clap_complete::CompleteEnv::with_factory(pb_command)
        .completer("pb")
        .complete();

    let args: Vec<String> = std::env::args().collect();
    // 裸 `pb`: 直接打一行简短 usage, 不进 less
    if args.len() == 1 {
        print!("{}", Cli::command().render_help());
        return ExitCode::SUCCESS;
    }
    // --help / -h / help: 完整帮助走 pager
    if matches!(
        args.get(1).map(String::as_str),
        Some("--help") | Some("-h") | Some("help")
    ) {
        let text = Cli::command().render_long_help().to_string();
        pager::page(&text, args.iter().any(|a| a == "--no-pager"));
        return ExitCode::SUCCESS;
    }
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{} {e:#}", "错误:".red().bold());
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    let no = cli.no_pager;
    match cli.command {
        Cmd::Add { content, file } => cmd_add(Store::load()?, content, file),
        Cmd::Ls => {
            let s = Store::load()?;
            let items: Vec<&store::Prompt> = s.list().iter().collect();
            pager::page(&render::ls_table(&items), no);
            Ok(())
        }
        Cmd::Show { id } => {
            let s = Store::load()?;
            print!("{}", render::show_text(&s.get(&id)?));
            Ok(())
        }
        Cmd::Copy { id, id_only, osc, local } => cmd_copy(Store::load()?, &id, id_only, osc, local),
        Cmd::Edit { id } => cmd_edit(Store::load()?, &id),
        Cmd::Rm { id, yes } => cmd_rm(Store::load()?, &id, yes),
        Cmd::Prune { yes } => cmd_prune(Store::load()?, yes),
        Cmd::Search { keywords } => {
            let s = Store::load()?;
            let kws: Vec<String> = keywords.iter().map(|k| k.to_lowercase()).collect();
            let hits: Vec<&store::Prompt> = s
                .list()
                .iter()
                .filter(|p| {
                    let hay = p.content.to_lowercase();
                    kws.iter().all(|k| hay.contains(k))
                })
                .collect();
            pager::page(&render::ls_table(&hits), no);
            Ok(())
        }
        Cmd::Completions { shell } => cmd_completions(shell),
    }
}

fn cmd_add(mut s: Store, content: Option<String>, file: Option<PathBuf>) -> Result<()> {
    let raw = if let Some(c) = content {
        c
    } else if let Some(f) = file {
        if f == PathBuf::from("-") {
            read_stdin()?
        } else {
            std::fs::read_to_string(&f).with_context(|| format!("读取文件失败: {}", f.display()))?
        }
    } else if !std::io::stdin().is_terminal() {
        read_stdin()?
    } else {
        edit_text(String::new())?
    };

    let content = raw.trim_end().to_string();
    if content.trim().is_empty() {
        bail!("内容为空, 未保存");
    }
    let p = s.add(&content)?;
    println!(
        "{} 已保存 [{}] —— 复制全文: `pb copy {}`",
        "✔".green().bold(),
        p.id.green(),
        p.id
    );
    Ok(())
}

fn read_stdin() -> Result<String> {
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .context("读取 stdin 失败")?;
    Ok(buf)
}

/// 编辑器解析优先级: PB_EDITOR > VISUAL > EDITOR > vi
fn editor_cmd() -> Vec<String> {
    for key in ["PB_EDITOR", "VISUAL", "EDITOR"] {
        if let Some(v) = std::env::var_os(key) {
            let parts: Vec<String> = v
                .to_string_lossy()
                .split_whitespace()
                .map(str::to_owned)
                .collect();
            if !parts.is_empty() {
                return parts;
            }
        }
    }
    vec!["vi".to_string()]
}

/// 在编辑器里编辑一段文本, 返回编辑后的内容
fn edit_text(initial: String) -> Result<String> {
    let mut tf = tempfile::Builder::new()
        .suffix(".md")
        .tempfile()
        .context("无法创建临时文件")?;
    tf.write_all(initial.as_bytes()).context("写入临时文件失败")?;
    tf.flush()?;

    let cmd = editor_cmd();
    let cmd_line = cmd.join(" ");
    let (prog, rest) = (cmd.first().cloned().unwrap_or_default(), &cmd[1..]);
    let status = std::process::Command::new(prog)
        .args(rest)
        .arg(tf.path())
        .status()
        .with_context(|| {
            format!("打开编辑器失败: `{cmd_line}` (可用 PB_EDITOR/$VISUAL/$EDITOR 指定, 如 set -Ux PB_EDITOR hx)")
        })?;
    if !status.success() {
        bail!("编辑器 `{cmd_line}` 非正常退出, 已放弃");
    }
    std::fs::read_to_string(tf.path()).context("读取编辑结果失败")
}

/// 用 COMPLETE 协议让自身重新执行来输出注册脚本(动态 id 补全需要脚本回调 pb)
fn cmd_completions(shell: clap_complete::Shell) -> Result<()> {
    let exe = std::env::current_exe().context("无法定位当前可执行文件")?;
    let status = std::process::Command::new(&exe)
        .env("COMPLETE", shell.to_string())
        .status()
        .with_context(|| format!("启动 {exe:?} 生成补全脚本失败"))?;
    if !status.success() {
        bail!("生成 {shell} 补全脚本失败 (exit {:?})", status.code());
    }
    Ok(())
}

fn cmd_copy(s: Store, query: &str, id_only: bool, osc: bool, local: bool) -> Result<()> {
    let p = s.get(query)?;
    let payload = if id_only { p.id.clone() } else { p.content.clone() };
    if osc && local {
        bail!("--osc 与 --local 不能同时使用");
    }

    let via: String;
    let ssh = !local && osc52::is_ssh();
    if osc || ssh {
        osc52::copy(&payload)?;
        via = if ssh {
            "OSC52 → 本地终端剪贴板(检测到 SSH)".to_string()
        } else {
            "OSC52 → 终端剪贴板".to_string()
        };
    } else {
        match clip::copy_text(&payload) {
            Ok(()) => via = "系统剪贴板".to_string(),
            Err(_) if std::io::stdout().is_terminal() => {
                // 本地会话但剪贴板服务不可用: 退而求其次走 OSC52
                osc52::copy(&payload)?;
                via = "OSC52 → 终端剪贴板(系统剪贴板不可用)".to_string();
            }
            Err(e) => return Err(e),
        }
    }

    let what = if id_only { "id" } else { "内容" };
    println!(
        "{} 已复制 [{}] 的{} → {} ({} 字符)",
        "✔".green().bold(),
        p.id,
        what,
        via,
        payload.chars().count()
    );
    Ok(())
}

fn cmd_edit(mut s: Store, query: &str) -> Result<()> {
    let p = s.get(query)?;
    let edited = edit_text(p.content.clone())?;
    let edited = edited.trim_end().to_string();
    if edited == p.content {
        println!("({}) 内容未修改", p.id);
        return Ok(());
    }
    let updated = s.update(&p.id, &edited)?;
    println!("{} 已更新 [{}]", "✔".green().bold(), updated.id);
    Ok(())
}

fn cmd_rm(mut s: Store, query: &str, yes: bool) -> Result<()> {
    let p = s.get(query)?;
    let stdin_tty = std::io::stdin().is_terminal();
    if !yes && stdin_tty {
        let ok = dialoguer::Confirm::new()
            .with_prompt(format!("删除 [{}] 这条提示词 ?", p.id))
            .default(false)
            .interact()
            .context("确认失败")?;
        if !ok {
            println!("已取消");
            return Ok(());
        }
    }
    s.remove(&p.id)?;
    println!("{} 已删除 [{}]", "✔".green().bold(), p.id);
    Ok(())
}

fn cmd_prune(mut s: Store, yes: bool) -> Result<()> {
    let n = s.list().len();
    if n == 0 {
        println!("(本来就是空的)");
        return Ok(());
    }
    if !yes {
        if !std::io::stdin().is_terminal() {
            bail!("非交互环境清空 {} 条需显式确认: `pb prune --yes`", n);
        }
        let ok = dialoguer::Confirm::new()
            .with_prompt(format!("清空全部 {} 条提示词?", n))
            .default(false)
            .interact()
            .context("确认失败")?;
        if !ok {
            println!("已取消");
            return Ok(());
        }
    }
    let n = s.clear()?;
    println!("{} 已清空 {} 条提示词", "✔".green().bold(), n);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(id: &str, content: &str) -> store::Prompt {
        store::Prompt {
            id: id.into(),
            content: content.into(),
            created: String::new(),
            updated: String::new(),
        }
    }

    #[test]
    fn ids_completion_filters_by_prefix() {
        let prompts = vec![
            p("f7q9m2", "把下面的文本翻译成英文"),
            p("rtnp73", "请以资深工程师视角 review 以下代码"),
            p("abc123", "其他"),
        ];
        let c = ids_candidates(&prompts, OsStr::new("f7"));
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].get_value(), "f7q9m2");
        assert_eq!(
            c[0].get_help().map(|h| h.to_string()),
            Some("把下面的文本翻译成英文".to_string())
        );
        let all = ids_candidates(&prompts, OsStr::new(""));
        assert_eq!(all.len(), 3);
    }
}
