# prompt-board (pb)

English | [中文](README.zh.md)

pb is a CLI (command-line tool) for the AI prompts you reuse: store them in a local data file, browse them in less, and copy one to the clipboard with a single command. When you are SSHed into a remote machine, the copy still lands on your local terminal's clipboard.

## Install

```sh
cargo install --path .        # installs the pb binary into ~/.cargo/bin
```

Requires a Rust toolchain of 1.85 or newer (edition 2024). Listing relies on `less`; writing to the system clipboard on a local machine needs an X11 or Wayland session. Over SSH pb does not touch the system clipboard, see "Clipboard and SSH (OSC52)".

## Quick start

```sh
pb add                        # open the editor, paste the prompt, save and quit
pb add -c "请把以下内容翻译成地道英文, 保持原意: …"
printf '你是资深 DBA, 帮我优化下面的 SQL:\n' | pb add
pb ls                         # browse in less, q quits
pb copy ceh                   # copy the text; --id copies the id itself
```

Actual `pb ls` output:

```text
ID      内容                                         更新时间
cehj2v  请把以下内容翻译成地道英文, 保持原意: …      2026-10-25 20:35
tzafku  你是资深 DBA, 帮我优化下面的 SQL 并解释索引  2026-10-25 20:35

共 2 条 · 详情 `pb show <id>` · 复制 `pb copy <id>` · id 支持唯一前缀, q 退出分页
```

## Commands

| Command | Description |
|---|---|
| `pb add` | Save a new prompt. Opens your editor by default; `-c` takes the text inline, `-f` reads a file, a pipe supplies it from stdin |
| `pb ls` | List all prompts through less |
| `pb show <id>` | Print the full text to stdout |
| `pb copy <id>` | Copy the text; `--id` copies the id itself; `--osc` and `--local` pick the channel |
| `pb edit <id>` | Edit the text in your editor |
| `pb rm <id>` | Delete; asks for confirmation in an interactive terminal, `-y` skips |
| `pb prune` | Delete all prompts; asks for confirmation, `-y` skips it |
| `pb search <关键词>…` | Search the texts; several keywords are ANDed together |
| `pb completions <shell>` | Print the shell completion registration script |

## Prompt ids

- Always 6 characters, from an alphabet that drops the confusable `0 o 1 l i`; lowercase plus digits with no separators, so a double click selects the whole id (in most terminals).
- `show/copy/edit/rm` accept a unique prefix: `pb copy 4d7` means `pb copy 4d7kqm`. When a prefix hits several ids, pb refuses and lists them.
- `pb copy <id> --id` copies the id itself, handy for pointing at one entry in a message.
- Pressing Tab lists every id live, each annotated with a one-line preview of its text.

## Clipboard and SSH (OSC52)

`pb copy` picks a channel by session:

- Local graphical session: the system clipboard (arboard); on failure it falls back to `wl-copy`, `xclip`, `xsel` in turn.
- SSH session (detected via `SSH_CONNECTION`, `SSH_CLIENT`, or `SSH_TTY`): pb base64-encodes the text into an OSC52 escape sequence and writes it straight to `/dev/tty`; your local terminal puts it on the clipboard. No X forwarding is needed, and a redirected stdout does not stop delivery.
- Local session, clipboard service unavailable, stdout a terminal: falls back to OSC52 automatically.

Whether OSC52 arrives depends on your local terminal's support: iTerm2, kitty, WezTerm, and Windows Terminal work; PuTTY needs its option enabled. Inside tmux pb wraps the sequence in a DCS passthrough layer; if tmux swallows it anyway, set `set -g allow-passthrough on`. The payload travels through the terminal, so it is capped at 100 KB by default (`PB_OSC52_MAX` adjusts it; over-cap content is truncated at a character boundary). To force a channel, use `--osc` and `--local`.

## Shell completion

```sh
fish:   pb completions fish > ~/.config/fish/completions/pb.fish
zsh:    pb completions zsh  > ~/.zsh/completions/_pb   # fpath must include this dir
bash:   pb completions bash > ~/.local/share/bash-completion/completions/pb
```

The generated script calls pb back at completion time: every Tab press computes candidates live, so nothing needs regenerating after you add or delete prompts, as long as `pb` is on your PATH. Subcommands, options, and `<ID>` arguments are all covered.

## Configuration

| Variable | Purpose |
|---|---|
| `PB_EDITOR` / `VISUAL` / `EDITOR` | The first non-empty one wins as an editor command; `vi` when none is set |
| `PB_DATA_FILE` | Override the data file path |
| `PB_OSC52_MAX` | OSC52 payload cap, 100000 bytes by default |
| `PAGER` | Override the pager; the default is `less -R -F -X` |

The global `--no-pager` option turns paging off. Prompts live in `~/.local/share/prompt-board/prompts.json`: pretty-printed JSON you can hand-edit; writes go to a `.tmp` file first and then rename, atomically.

## Development

```sh
cargo build --release   # the binary target is pb
cargo test              # storage, completion candidates, OSC52 boundaries
```

Bilingual docs follow the contract in `docs/i18n/pairing-contract.md`; the gate is `scripts/pairing.py`: `write` records a confirmation, `check` verifies consistency.
