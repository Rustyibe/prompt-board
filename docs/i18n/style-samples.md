# 语体样例

翻译时对着同体裁样例比；样例与成文规则冲突时，样例优先。术语表优先级高于两者。

以下两段摘自 README 配对，经作者确认入册（2026-10-25）。新增或修正样例同样要经过评审；样例与术语表冲突时，改样例。

## ① 机制说明

> Always 6 characters, from an alphabet that drops the confusable `0 o 1 l i`; lowercase plus digits with no separators, so a double click selects the whole id (in most terminals).

固定 6 位，字母表剔除了易混淆的 `0 o 1 l i`；纯小写加数字，中间没有分隔符，双击即可整段选中（多数终端）。

为什么这样处理：英文的分号长句在中文里按因果顺序拆成三个短句，信息量不减；限定成分 "(in most terminals)" 用句末括注（多数终端）而非「大多数终端下都能」，避免拖尾。

## ② 操作步骤

> Requires a Rust toolchain of 1.85 or newer (edition 2024). Listing relies on `less`; writing to the system clipboard on a local machine needs an X11 or Wayland session.

需要 Rust 1.85 以上的工具链（edition 2024）。列表浏览依赖 `less`；本机的剪贴板写入需要 X11 或 Wayland 会话。

为什么这样处理：依赖陈述保持祈使外的直陈语气，两个分号并列拆成中文分号；`1.85`、`edition 2024`、`less` 等标识符原样，数字不用全角。

## 提炼清单（随批准样例积累）

- 语域：机构式技术文风，主谓完整、语气确定。
- 施动者：中文把抽象主语换成明确的（pb、终端、系统剪贴板）。
- 段落：一段一个意思；段落边界可以和源文不同。
- 标识符：命令、选项、环境变量、路径保持行内代码原样。
