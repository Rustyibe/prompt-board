#!/usr/bin/env python3
"""英文/简体中文文档配对的一致性门禁。

一个配对是三个同级文件：``foo.md``、``foo.zh.md``、``foo.i18n.yaml``。yaml
记录里按标题小节保存两侧正文 block 的哈希。门禁证明的是“在这份确切内容上，
配对已被确认一致”，它证明不了这个确认本身是对的。详见 references/pairing-contract.md。

命令：
  list [--root DIR]         列出范围内每个配对的状态：ok/missing/out-of-sync
  check [PATH...]           校验配对（默认全语料）；任一失败即退出码 1
  write PATH... | --all     重新记录已确认的配对
  brief PATH                打印某个失配配对里发生改动的小节
"""

from __future__ import annotations

import argparse
import difflib
import hashlib
import json
import os
import re
import subprocess
import sys
from dataclasses import dataclass, field
from pathlib import Path

CONFIG_NAME = ".doc-pairing.json"
SKIP_DIRS = {
    ".git", ".hg", ".svn", "node_modules", "vendor", "dist", "build", "out",
    ".venv", "venv", "__pycache__", ".next", ".cache", "target",
}
FENCE_RE = re.compile(r"^(?P<indent>\s*)(?P<mark>`{3,}|~{3,})(?P<info>.*)$")
HEADING_RE = re.compile(r"^(?P<hashes>#{1,6})\s+(?P<text>.*?)\s*#*\s*$")
LINK_RE = re.compile(r"!?\[[^\]]*\]\((?P<dest>[^)\s]+)(?:\s+\"[^\"]*\")?\)")
LIST_RE = re.compile(r"^(?P<indent>\s*)(?P<bullet>[-*+]|\d+[.)])\s+")
TABLE_DELIM_RE = re.compile(r"^\s*\|?\s*:?-{2,}:?\s*(\|\s*:?-{2,}:?\s*)*\|?\s*$")
SWITCHER_EN_RE = re.compile(r"^\s*English\s*\|\s*\[[^\]]+\]\([^)]*\.zh\.md\)\s*$")
SWITCHER_ZH_RE = re.compile(r"^\s*\[[^\]]+\]\((?![a-z]+:)[^)]*\.md\)\s*\|\s*中文\s*$")
RECORD_KEY_RE = re.compile(r"^(/[^\s:#]*):$")
RECORD_HASH_RE = re.compile(r"^ {2}(en|zh): ([0-9a-f]{16})$")


# --------------------------------------------------------------------------
# Markdown 模型
# --------------------------------------------------------------------------


@dataclass
class Block:
    """一个顶层 block：围栏之外的连续非空行。"""

    lines: list[str]
    line_no: int
    is_code: bool = False


@dataclass
class Section:
    """一个由标题划出的小节，以及它拥有的 block。"""

    key: str
    heading_level: int | None
    blocks: list[Block] = field(default_factory=list)


def read_lines(path: Path) -> list[str]:
    return path.read_text(encoding="utf-8").splitlines()


def slugify(text: str) -> str:
    """近似算出英文标题的 GitHub slug。"""
    text = text.strip().lower()
    text = re.sub(r"[`*_\[\]()!?.,:;'\"\\/]", "", text)
    text = re.sub(r"[^a-z0-9\u4e00-\u9fff\s-]", "", text)
    return re.sub(r"[\s]+", "-", text).strip("-")


def section_key(stack: list[tuple[int, str]]) -> str:
    return "/" + "/".join(slug for _, slug in stack) if stack else "/"


def parse_sections(lines: list[str]) -> list[Section]:
    """把文档切成标题小节；代码围栏内部的标题不会开启新小节。"""
    sections: list[Section] = []
    stack: list[tuple[int, str]] = []
    seen: dict[str, int] = {}
    current = Section(key="/", heading_level=None)
    fence: tuple[str, int] | None = None
    block: list[str] = []
    block_line = 0

    def flush() -> None:
        nonlocal block
        if block:
            current.blocks.append(Block(block, block_line))
            block = []

    for number, raw in enumerate(lines, start=1):
        fence_match = FENCE_RE.match(raw)
        if fence is None and fence_match:
            if not block:
                block_line = number
            block.append(raw)
            fence = (fence_match.group("mark")[0], len(fence_match.group("mark")))
            continue
        if fence is not None:
            if not block:
                block_line = number
            block.append(raw)
            if (
                fence_match
                and fence_match.group("mark")[0] == fence[0]
                and len(fence_match.group("mark")) >= fence[1]
                and not fence_match.group("info").strip()
            ):
                current.blocks.append(Block(block, block_line, is_code=True))
                block = []
                fence = None
            continue

        heading = HEADING_RE.match(raw)
        if heading:
            flush()
            sections.append(current)
            level = len(heading.group("hashes"))
            while stack and stack[-1][0] >= level:
                stack.pop()
            stack.append((level, slugify(heading.group("text"))))
            base = section_key(stack)
            count = seen.get(base, 0) + 1
            seen[base] = count
            key = base if count == 1 else f"{base}~{count}"
            current = Section(key=key, heading_level=level)
            continue

        if not raw.strip():
            flush()
            continue
        if not block:
            block_line = number
        block.append(raw)

    flush()
    sections.append(current)
    return sections


def block_text(block: Block) -> str:
    return "\n".join(line.rstrip() for line in block.lines).strip("\n")


def section_hash(section: Section) -> str:
    """哈希该小节的正文 block；围栏代码在别处做逐字节比对。"""
    payload = "\n\n".join(t for t in (block_text(b) for b in section.blocks if not b.is_code) if t)
    return hashlib.sha256(payload.encode("utf-8")).hexdigest()[:16]


def record_entries(sections: list[Section]) -> dict[str, str]:
    return {
        section.key: section_hash(section)
        for section in sections
        if any(not block.is_code and block_text(block) for block in section.blocks)
    }


# --------------------------------------------------------------------------
# 结构签名
# --------------------------------------------------------------------------


def table_shape(block: Block) -> tuple[int, int] | None:
    lines = block.lines
    if len(lines) < 2 or "|" not in lines[0] or not TABLE_DELIM_RE.match(lines[1]):
        return None
    columns = len([c for c in re.split(r"(?<!\\)\|", lines[0].strip().strip("|"))])
    return (len(lines) - 2, columns)


def list_shape(block: Block) -> tuple[str, int, int] | None:
    items: list[list[str]] = []
    kind = ""
    start = 1
    for line in block.lines:
        match = LIST_RE.match(line)
        if match and not match.group("indent"):
            marker = match.group("bullet")
            this_kind = "ol" if marker[0].isdigit() else "ul"
            if not items:
                kind = this_kind
                start = int(marker[:-1]) if this_kind == "ol" else 1
            if this_kind != kind:
                return None
            items.append([line])
        elif items:
            items[-1].append(line)
    return (kind, start, len(items)) if items else None


def strip_switcher(lines: list[str]) -> list[str]:
    return [line for line in lines if not (SWITCHER_EN_RE.match(line) or SWITCHER_ZH_RE.match(line))]


def normalize_link(dest: str, chinese_side: bool) -> str:
    if re.match(r"^[a-z][a-z0-9+.-]*:", dest) or dest.startswith("#"):
        return dest
    if chinese_side:
        return dest.replace(".zh.md", ".md")
    return dest


def structural_signature(lines: list[str], chinese_side: bool) -> dict[str, list]:
    body = strip_switcher(lines)
    sections = parse_sections(body)
    signature: dict[str, list] = {"headings": [], "fences": [], "tables": [], "lists": [], "links": []}
    for section in sections:
        for block in section.blocks:
            if block.is_code:
                info = FENCE_RE.match(block.lines[0]).group("info").strip()  # type: ignore[union-attr]
                signature["fences"].append((info, "\n".join(block.lines[1:-1])))
                continue
            shape = table_shape(block)
            if shape:
                signature["tables"].append(shape)
                continue
            shape_list = list_shape(block)
            if shape_list:
                signature["lists"].append(shape_list)
        if section.heading_level:
            signature["headings"].append(section.heading_level)
    for line in body:
        for match in LINK_RE.finditer(line):
            signature["links"].append(normalize_link(match.group("dest"), chinese_side))
    return signature


# --------------------------------------------------------------------------
# 配对模型
# --------------------------------------------------------------------------


@dataclass
class Pair:
    source: Path
    zh: Path
    meta: Path
    root: Path

    @property
    def stem(self) -> str:
        return str(self.source.relative_to(self.root))


def pair_paths(source: Path) -> tuple[Path, Path]:
    return (
        source.with_name(source.name[: -len(".md")] + ".zh.md"),
        source.with_name(source.name[: -len(".md")] + ".i18n.yaml"),
    )


def load_config(root: Path) -> dict:
    config_path = root / CONFIG_NAME
    if config_path.is_file():
        config = json.loads(config_path.read_text(encoding="utf-8"))
    else:
        config = {
            "roots": ["README.md", "docs", "*/README.md"],
            "exclude": [],
            "english_only": ["AGENTS.md", "**/AGENTS.md"],
            "generated": [],
        }
    config.setdefault("contract", "docs/i18n/pairing-contract.md")
    return config


def matches_pattern(path: Path, pattern: str) -> bool:
    if path.match(pattern):
        return True
    # ``**/x`` 也应覆盖根目录下的匹配，Path.match 做不到这一点。
    return pattern.startswith("**/") and path.match(pattern[3:])


def discover(root: Path, config: dict) -> list[Path]:
    exclude = config.get("exclude", [])
    english_only = config.get("english_only", [])
    found: set[Path] = set()
    for pattern in config["roots"]:
        for candidate in root.glob(pattern):
            if candidate.is_file() and candidate.suffix == ".md":
                found.add(candidate)
            elif candidate.is_dir():
                found.update(p for p in candidate.rglob("*.md") if p.is_file())
    result = []
    for path in sorted(found):
        rel = path.relative_to(root)
        if any(part in SKIP_DIRS for part in rel.parts):
            continue
        if path.name.endswith(".zh.md"):
            continue
        if any(matches_pattern(rel, pat) for pat in exclude + english_only):
            continue
        result.append(path)
    return result


def read_record(path: Path) -> dict[str, dict[str, str]]:
    entries: dict[str, dict[str, str]] = {}
    if not path.is_file():
        return entries
    key = None
    for line in read_lines(path):
        match = RECORD_KEY_RE.match(line)
        if match:
            key = match.group(1)
            entries[key] = {}
            continue
        match = RECORD_HASH_RE.match(line)
        if match and key:
            entries[key][match.group(1)] = match.group(2)
    return entries


def render_record(pair: Pair, entries: dict[str, dict[str, str]], contract: str) -> str:
    lines = [
        f"# {pair.stem} 的双语配对一致性记录（见 {contract}）。",
        "# 按标题小节记录：该小节中围栏代码之外的英文与中文 block 的哈希。",
        "# 改动任意一侧后，请同步另一侧并重新记录：",
        f"#   python3 scripts/pairing.py write {pair.stem}",
    ]
    for key, hashes in entries.items():
        lines.append(f"{key}:")
        lines.append(f"  en: {hashes['en']}")
        lines.append(f"  zh: {hashes['zh']}")
    return "\n".join(lines) + "\n"


EMPTY_HASH = hashlib.sha256(b"").hexdigest()[:16]


def has_prose(section: Section | None) -> bool:
    return bool(section) and any(not block.is_code and block_text(block) for block in section.blocks)


def align(en_sections: list[Section], zh_sections: list[Section]) -> list[tuple[str, Section | None, Section | None]]:
    """按序号配对两侧小节；记录条目的 key 取英文标题的 slug。"""
    keys: list[str] = []
    for index in range(max(len(en_sections), len(zh_sections))):
        if index < len(en_sections):
            keys.append(en_sections[index].key)
        else:
            keys.append(f"/~unmatched-{index}")
    return [
        (keys[index], en_sections[index] if index < len(en_sections) else None,
         zh_sections[index] if index < len(zh_sections) else None)
        for index in range(len(keys))
    ]


def compute_entries(pair: Pair) -> dict[str, dict[str, str]]:
    aligned = align(parse_sections(read_lines(pair.source)), parse_sections(read_lines(pair.zh)))
    entries: dict[str, dict[str, str]] = {}
    for key, en, zh in aligned:
        if not has_prose(en) and not has_prose(zh):
            continue
        entries[key] = {
            "en": section_hash(en) if en else EMPTY_HASH,
            "zh": section_hash(zh) if zh else EMPTY_HASH,
        }
    return entries


# --------------------------------------------------------------------------
# 校验
# --------------------------------------------------------------------------


def check_switchers(pair: Pair) -> list[str]:
    problems = []
    en_lines = read_lines(pair.source)
    zh_lines = read_lines(pair.zh)
    zh_switcher = f"[English]({pair.source.name}) | 中文"
    en_switcher = f"English | [中文]({pair.zh.name})"
    if not any(SWITCHER_ZH_RE.match(line) for line in zh_lines[:6]):
        problems.append(f"{pair.zh.relative_to(pair.root)}: H1 之后缺少语言切换行：{zh_switcher}")
    if not any(SWITCHER_EN_RE.match(line) for line in en_lines[:6]):
        problems.append(f"{pair.source.relative_to(pair.root)}: H1 之后缺少语言切换行：{en_switcher}")
    return problems


def check_record(pair: Pair) -> list[str]:
    recorded = read_record(pair.meta)
    computed = compute_entries(pair)
    problems = []
    for key, hashes in computed.items():
        if key not in recorded:
            problems.append(f"小节 {key} 尚未记录：先翻译，再重新记录")
            continue
        en_changed = recorded[key].get("en") != hashes["en"]
        zh_changed = recorded[key].get("zh") != hashes["zh"]
        if en_changed and zh_changed:
            problems.append(f"小节 {key}: 两侧都改过；先手工合并，再重新记录")
        elif en_changed:
            problems.append(f"小节 {key}: 英文有改动，中文对侧未同步")
        elif zh_changed:
            problems.append(f"小节 {key}: 中文有改动，英文对侧未同步")
    for key in recorded:
        if key not in computed:
            problems.append(f"记录中的小节 {key} 已经不含正文")
    return problems


def check_structure(pair: Pair) -> list[str]:
    en = structural_signature(read_lines(pair.source), chinese_side=False)
    zh = structural_signature(read_lines(pair.zh), chinese_side=True)
    problems = []
    for kind in ("headings", "fences", "tables", "lists", "links"):
        if en[kind] == zh[kind]:
            continue
        if kind == "fences":
            en_set, zh_set = set(map(tuple, en[kind])), set(map(tuple, zh[kind]))
            missing = en_set - zh_set
            extra = zh_set - en_set
            detail = ""
            if missing:
                detail += f"；中文侧缺少：{sorted(i for i, _ in missing)[:5]}"
            if extra:
                detail += f"；仅中文侧有：{sorted(i for i, _ in extra)[:5]}"
            problems.append(f"代码围栏不一致{detail}")
        elif kind == "links":
            missing = [x for x in en[kind] if x not in zh[kind]]
            extra = [x for x in zh[kind] if x not in en[kind]]
            if missing:
                problems.append(f"中文侧缺少链接：{missing[:5]}")
            if extra:
                problems.append(f"仅中文侧有的链接：{extra[:5]}")
        else:
            problems.append(f"{kind} 不一致：en={en[kind][:8]} zh={zh[kind][:8]}")
    return problems


def check_pair(pair: Pair) -> list[str]:
    if not pair.zh.is_file():
        return [f"配对不完整：缺少 {pair.zh.name}"]
    if not pair.meta.is_file():
        return [f"配对尚未记录：先把 {pair.zh.name} 定稿，再执行 write {pair.stem}"]
    problems: list[str] = []
    problems += check_switchers(pair)
    problems += check_record(pair)
    problems += check_structure(pair)
    return problems


def status(pair: Pair) -> str:
    if not pair.zh.is_file():
        return "missing"
    if not pair.meta.is_file():
        return "unrecorded"
    return "ok" if not (check_record(pair) or check_structure(pair) or check_switchers(pair)) else "out-of-sync"


# --------------------------------------------------------------------------
# 简报
# --------------------------------------------------------------------------


def git(root: Path, *args: str) -> str:
    result = subprocess.run(
        ["git", "-C", str(root), *args], capture_output=True, text=True, check=False
    )
    if result.returncode != 0:
        raise RuntimeError(result.stderr.strip() or "git 命令执行失败")
    return result.stdout


def last_confirmed_rev(pair: Pair) -> str | None:
    try:
        rev = git(pair.root, "log", "-1", "--format=%H", "--", str(pair.meta.relative_to(pair.root))).strip()
    except RuntimeError:
        return None
    return rev or None


def file_at(pair: Pair, path: Path, rev: str) -> str:
    rel = str(path.relative_to(pair.root))
    try:
        return git(pair.root, "show", f"{rev}:{rel}")
    except RuntimeError:
        return ""


def section_prose(text: str) -> list[str]:
    """按文档顺序返回每个小节的正文，与对侧按序号对齐。"""
    return [
        "\n".join(block_text(block) for block in section.blocks if not block.is_code).strip()
        for section in parse_sections(text.splitlines())
    ]


def brief(pair: Pair) -> str:
    rev = last_confirmed_rev(pair)
    recorded = read_record(pair.meta)
    computed = compute_entries(pair)
    aligned = align(parse_sections(read_lines(pair.source)), parse_sections(read_lines(pair.zh)))
    keys = [key for key, _, _ in aligned]
    out = [f"# 简报：{pair.stem}", ""]
    out.append(f"上次确认的版本：{rev or '（无；请把每个小节都当作新增）'}")
    changed = [
        key for key in keys
        if key in computed
        and (
            key not in recorded
            or recorded[key].get("en") != computed[key]["en"]
            or recorded[key].get("zh") != computed[key]["zh"]
        )
    ]
    en_edited = [k for k in changed if k in recorded and recorded[k].get("en") != computed[k]["en"]]
    zh_edited = [k for k in changed if k in recorded and recorded[k].get("zh") != computed[k]["zh"]]
    if not changed:
        out.append("自上次确认版本以来没有正文改动。")
        if not rev:
            out.append("记录文件还没有提交，因此没有可对比的确认版本。")
        return "\n".join(out)
    if en_edited and not zh_edited:
        out.append("改动的一侧：en")
    elif zh_edited and not en_edited:
        out.append("改动的一侧：zh")
    elif en_edited and zh_edited:
        out.append("改动的一侧：两侧都改过（需手工合并）")
    out.append("")
    old_en = section_prose(file_at(pair, pair.source, rev)) if rev else []
    old_zh = section_prose(file_at(pair, pair.zh, rev)) if rev else []
    new_en = section_prose("\n".join(read_lines(pair.source)))
    new_zh = section_prose("\n".join(read_lines(pair.zh)))

    def at(values: list[str], key: str) -> str:
        index = keys.index(key)
        return values[index] if index < len(values) else ""

    for key in changed:
        out.append(f"## {key}")
        for side, old, new in (("en", old_en, new_en), ("zh", old_zh, new_zh)):
            diff = list(
                difflib.unified_diff(
                    (at(old, key) + "\n").splitlines(),
                    (at(new, key) + "\n").splitlines(),
                    fromfile=f"{side}（已确认）",
                    tofile=f"{side}（当前）",
                    lineterm="",
                    n=2,
                )
            )
            if diff:
                out.append(f"### {side} 侧 diff")
                out.extend(["```diff", *diff, "```"])
        out.append("### 对侧当前正文")
        counterpart = new_zh if (key in en_edited or not zh_edited) else new_en
        out.extend(["```markdown", at(counterpart, key), "```"])
        out.append("")
    out.append("只改上面这些 hunk。其余位置的措辞已评审过，要保留，改完再执行 `write`。")
    return "\n".join(out)


# --------------------------------------------------------------------------
# 命令行
# --------------------------------------------------------------------------


def resolve(root: Path, targets: list[str]) -> list[Pair]:
    pairs = []
    for target in targets:
        source = (root / target).resolve()
        if source.name.endswith(".zh.md"):
            source = source.with_name(source.name[: -len(".zh.md")] + ".md")
        elif source.name.endswith(".i18n.yaml"):
            source = source.with_name(source.name[: -len(".i18n.yaml")] + ".md")
        if not source.is_file():
            raise SystemExit(f"不是文档：{target}")
        zh, meta = pair_paths(source)
        pairs.append(Pair(source=source, zh=zh, meta=meta, root=root))
    return pairs


def chinese_files(root: Path, config: dict) -> list[Path]:
    """配置的根目录下能找到的所有中文对侧文件。"""
    found: set[Path] = set()
    for pattern in config["roots"]:
        for candidate in root.glob(pattern):
            if candidate.is_file():
                if candidate.name.endswith(".zh.md"):
                    found.add(candidate)
            else:
                found.update(candidate.rglob("*.zh.md"))
    return sorted(found)


def corpus(root: Path, config: dict) -> tuple[list[Pair], list[Path]]:
    """返回配置根目录下的配对，以及没有英文源的中文文件。"""
    sources = discover(root, config)
    pairs = []
    for source in sources:
        zh, meta = pair_paths(source)
        pairs.append(Pair(source=source, zh=zh, meta=meta, root=root))

    exclude = config.get("exclude", []) + config.get("english_only", [])
    orphans = []
    for path in chinese_files(root, config):
        rel = path.relative_to(root)
        if any(part in SKIP_DIRS for part in rel.parts):
            continue
        base = path.with_name(path.name[: -len(".zh.md")] + ".md")
        if base.is_file():
            continue
        if any(matches_pattern(base.relative_to(root), pattern) for pattern in exclude):
            continue
        orphans.append(path)
    return pairs, orphans


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--root", default=".", help="项目根目录（默认：当前目录）")
    sub = parser.add_subparsers(dest="command", required=True)

    sub.add_parser("list", help="列出范围内每篇文档的配对状态")

    check = sub.add_parser("check", help="校验配对；失败时退出码为 1")
    check.add_argument("paths", nargs="*")

    write = sub.add_parser("write", help="重新记录已确认的配对")
    write.add_argument("paths", nargs="*")
    write.add_argument("--all", action="store_true", help="重新记录整个语料")

    brief_cmd = sub.add_parser("brief", help="打印某个配对里发生改动的小节")
    brief_cmd.add_argument("path")

    args = parser.parse_args(argv)
    root = Path(args.root).resolve()
    config = load_config(root)

    if args.command == "list":
        pairs, orphans = corpus(root, config)
        for pair in pairs:
            print(f"{status(pair):12} {pair.stem}")
        for path in orphans:
            print(f"{'orphan':12} {path.relative_to(root)}")
        return 0

    if args.command == "check":
        if args.paths:
            pairs, orphans = resolve(root, args.paths), []
        else:
            pairs, orphans = corpus(root, config)
        failed = 0
        for pair in pairs:
            problems = check_pair(pair)
            if problems:
                failed += 1
                print(f"FAIL {pair.stem}")
                for problem in problems:
                    print(f"  - {problem}")
        for path in orphans:
            failed += 1
            print(f"FAIL {path.relative_to(root)}")
            print("  - 孤立译文：没有同名的英文源文件")
        if failed:
            print(f"\n{len(pairs) + len(orphans)} 个配对中，{failed} 个不一致。")
            return 1
        print(f"{len(pairs)} 个配对一致。")
        return 0

    if args.command == "write":
        if not args.paths and not args.all:
            parser.error("write 需要显式指定路径，或用 --all 处理整个语料")
        pairs = corpus(root, config)[0] if args.all else resolve(root, args.paths)
        for pair in pairs:
            if not pair.zh.is_file():
                print(f"SKIP {pair.stem}: 没有中文对侧")
                continue
            pair.meta.write_text(
                render_record(pair, compute_entries(pair), config.get("contract", "docs/i18n/pairing-contract.md")),
                encoding="utf-8",
            )
            print(f"已记录 {pair.stem}")
        return 0

    pair = resolve(root, [args.path])[0]
    print(brief(pair))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
