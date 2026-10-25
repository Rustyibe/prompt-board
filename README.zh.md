# prompt-board（pb）

[English](README.md) | 中文

pb 是一个存取常用 AI 提示词（prompt）的 CLI（命令行工具）：保存到本地数据文件，用 less 浏览，一条命令复制进剪贴板。当你 SSH 连到远程机器时，复制依然落回本地终端的剪贴板。

## 安装

```sh
cargo install --path .        # installs the pb binary into ~/.cargo/bin
```

需要 Rust 1.85 以上的工具链（edition 2024）。列表浏览依赖 `less`；本机的剪贴板写入需要 X11 或 Wayland 会话。SSH 场景不涉及系统剪贴板，见「剪贴板与 SSH（OSC52）」。

## 快速上手

```sh
pb add                        # open the editor, paste the prompt, save and quit
pb add -c "请把以下内容翻译成地道英文, 保持原意: …"
printf '你是资深 DBA, 帮我优化下面的 SQL:\n' | pb add
pb ls                         # browse in less, q quits
pb copy ceh                   # copy the text; --id copies the id itself
```

`pb ls` 的实际输出：

```text
ID      内容                                         更新时间
cehj2v  请把以下内容翻译成地道英文, 保持原意: …      2026-10-25 20:35
tzafku  你是资深 DBA, 帮我优化下面的 SQL 并解释索引  2026-10-25 20:35

共 2 条 · 详情 `pb show <id>` · 复制 `pb copy <id>` · id 支持唯一前缀, q 退出分页
```

## 命令

| 命令 | 说明 |
|---|---|
| `pb add` | 新增提示词。默认打开编辑器；`-c` 直接给正文，`-f` 读文件，管道输入从 stdin 读 |
| `pb ls` | 列出全部提示词，送入 less |
| `pb show <id>` | 全文直接输出到 stdout |
| `pb copy <id>` | 复制正文；`--id` 复制 id 本身；`--osc` 与 `--local` 选择通道 |
| `pb edit <id>` | 用编辑器修改正文 |
| `pb rm <id>` | 删除；交互终端里会确认，`-y` 跳过 |
| `pb search <关键词>…` | 搜索正文，多个关键词为 AND |
| `pb completions <shell>` | 输出自动补全（completion）注册脚本 |

## 提示词 id

- 固定 6 位，字母表剔除了易混淆的 `0 o 1 l i`；纯小写加数字，中间没有分隔符，双击即可整段选中（多数终端）。
- `show/copy/edit/rm` 都接受唯一前缀：`pb copy 4d7` 即 `pb copy 4d7kqm`。前缀撞上多条时，报错并列出候选 id。
- `pb copy <id> --id` 复制 id 本身，方便在消息里回指同一条提示词。
- 按 Tab 时由 pb 现场列出全部 id，附正文首行摘要作说明。

## 剪贴板与 SSH（OSC52）

`pb copy` 根据所在会话选择通道：

- 本地图形会话：写系统剪贴板（arboard），失败则依次回退 `wl-copy`、`xclip`、`xsel`。
- SSH 会话（检测到 `SSH_CONNECTION`、`SSH_CLIENT` 或 `SSH_TTY`）：把正文 base64 编码进 OSC52 转义序列，直接写入 `/dev/tty`，由你本地的终端把内容放进剪贴板。不需要 X 转发，stdout 被重定向也不影响送达。
- 本地会话但剪贴板服务不可用、且 stdout 是终端：自动回退 OSC52。

OSC52 能否送达取决于本地终端是否支持：iTerm2、kitty、WezTerm、Windows Terminal 可用，PuTTY 需要勾选对应选项。tmux 里 pb 会自动包一层 DCS passthrough；若序列被 tmux 吞掉，设置 `set -g allow-passthrough on`。内容经由终端传输，默认上限 100 KB（`PB_OSC52_MAX` 可调；超限会截断，且截断点落在字符边界上）。想显式指定通道就用 `--osc` 和 `--local`。

## 自动补全

```sh
fish:   pb completions fish > ~/.config/fish/completions/pb.fish
zsh:    pb completions zsh  > ~/.zsh/completions/_pb   # fpath must include this dir
bash:   pb completions bash > ~/.local/share/bash-completion/completions/pb
```

生成的脚本在运行时回调 pb：每次按 Tab 都现场计算候选，增删提示词后不必重新生成，前提是 `pb` 在 PATH 里。子命令、选项与 `<ID>` 参数都覆盖。

## 配置

| 变量 | 作用 |
|---|---|
| `PB_EDITOR` / `VISUAL` / `EDITOR` | 按顺序取第一个非空的编辑器命令，都缺省时用 `vi` |
| `PB_DATA_FILE` | 覆盖数据文件路径 |
| `PB_OSC52_MAX` | OSC52 内容上限，默认 100000 字节 |
| `PAGER` | 覆盖分页器，默认 `less -R -F -X` |

全局选项 `--no-pager` 关闭分页。数据存放在 `~/.local/share/prompt-board/prompts.json`：格式化 JSON，可以手改；写入先落 `.tmp` 再 rename，是原子操作。

## 开发

```sh
cargo build --release   # the binary target is pb
cargo test              # storage, completion candidates, OSC52 boundaries
```

双语文档按 `docs/i18n/pairing-contract.md` 的契约维护，门禁是 `scripts/pairing.py`：`write` 记录确认，`check` 校验一致性。
