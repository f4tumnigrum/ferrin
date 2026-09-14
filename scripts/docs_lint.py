#!/usr/bin/env python3
"""Check bilingual documentation coverage, links, anchors, and pending items."""
import os
from pathlib import Path
import re
import sys
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parent.parent
LINK = re.compile(r"\[[^\]]*\]\(([^)\s]*)\)")
HAN = re.compile(r"[\u3400-\u9fff]")
PENDING = re.compile(r"^\s*(?:[-*]\s+|\d+\.\s+|\|\s*)?(?:\[Pending verification\]|【待验证】)(?!\()")
PV = re.compile(r"PV-\d{3}")
ENTRY = re.compile(r"^\| (PV-\d{3}) \|.*\| (open|closed) \|$", re.M)
APPENDIX = Path("05-appendix/02-pending-verification.md")


def md_files(root):
    for base, dirs, files in os.walk(root):
        dirs[:] = sorted(d for d in dirs if d not in {"target", ".git", "node_modules"})
        for name in sorted(files):
            if name.endswith(".md"):
                yield Path(base) / name


def prose_lines(text):
    """Exclude fenced examples when interpreting Markdown structure."""
    fence = None
    for number, line in enumerate(text.splitlines(), 1):
        marker = re.match(r"^\s{0,3}(`{3,}|~{3,})", line)
        if marker:
            run = marker[1]
            if fence is None:
                fence = run
            elif run[0] == fence[0] and len(run) >= len(fence):
                fence = None
            continue
        if fence is None:
            yield number, line


def anchors(text):
    """GitHub-style ATX heading IDs, including duplicate-heading suffixes."""
    used = set()
    for _, line in prose_lines(text):
        match = re.match(r"^#{1,6}\s+(.+?)(?:\s+#+)?$", line)
        if not match:
            continue
        heading = re.sub(r"\[([^\]]+)\]\([^)]*\)", r"\1", match[1])
        slug = re.sub(r"[^\w\- ]", "", heading.lower()).replace(" ", "-")
        candidate, suffix = slug, 0
        while candidate in used:
            suffix += 1
            candidate = f"{slug}-{suffix}"
        used.add(candidate)
    used.update(re.findall(r'<(?:a|[hH][1-6])\s+[^>]*id=["\']([^"\']+)', text))
    return used


def check(root):
    root = Path(root).resolve()
    documents = {p: p.read_text(encoding="utf-8") for p in md_files(root)}
    errors = []
    editions = {"en": root / "docs", "zh-CN": root / "docs/zh-CN"}
    pages = {
        language: {p.relative_to(base) for p in documents if p.is_relative_to(base)
                   and (language != "en" or not p.is_relative_to(editions["zh-CN"]))}
        for language, base in editions.items()
    }
    for language, other in (("en", "zh-CN"), ("zh-CN", "en")):
        for missing in sorted(pages[other] - pages[language]):
            errors.append(f"{language}: missing translated page {missing}")
    for name in ("README.md", "README.zh-CN.md"):
        if root / name not in documents:
            errors.append(f"missing {name}")

    registries = {}
    for language, base in editions.items():
        path = base / APPENDIX
        if path not in documents:
            errors.append(f"missing appendix: {path.relative_to(root)}")
            registries[language] = {}
            continue
        entries = ENTRY.findall(documents[path])
        registries[language] = dict(entries)
        if len(entries) != len(registries[language]):
            errors.append(f"{language}: duplicate pending-verification ID")
    if registries["en"] != registries["zh-CN"]:
        errors.append("pending-verification IDs/statuses differ between editions")

    heading_ids = {p: anchors(text) for p, text in documents.items()}
    for path, text in documents.items():
        rel = path.relative_to(root)
        chinese = path.is_relative_to(editions["zh-CN"]) or rel == Path("README.zh-CN.md")
        language = "zh-CN" if chinese else "en"
        primary = rel == Path("README.md")
        primary = primary or (path.is_relative_to(editions["en"]) and not chinese)
        if primary and HAN.search(text):
            errors.append(f"{rel}: Chinese text in the primary English edition")
        for number, line in prose_lines(text):
            if PENDING.match(line) and path != editions[language] / APPENDIX:
                ids = PV.findall(line)
                if not ids:
                    errors.append(f"{rel}:{number}: pending item without PV-xxx reference")
                for item in ids:
                    if registries[language].get(item) != "open":
                        errors.append(f"{rel}:{number}: {item} is not registered as open in {language}")
            for match in LINK.finditer(line):
                target = match[1]
                url = urlsplit(target)
                if url.scheme or url.netloc:
                    continue
                resolved = (path.parent / unquote(url.path)).resolve() if url.path else path
                if not resolved.exists():
                    errors.append(f"{rel}:{number}: broken link {target}")
                    continue
                if url.fragment and resolved in heading_ids and unquote(url.fragment) not in heading_ids[resolved]:
                    errors.append(f"{rel}:{number}: broken anchor {target}")
                # Only the explicit language switch may cross documentation editions.
                switch = line.startswith("**English** |") or line.startswith("[English](")
                if not switch and resolved.is_relative_to(editions["en"]):
                    target_chinese = resolved.is_relative_to(editions["zh-CN"])
                    shared = resolved.is_relative_to(editions["en"] / "api")
                    if not shared and (chinese or primary) and chinese != target_chinese:
                        errors.append(f"{rel}:{number}: chapter link crosses language editions: {target}")
    counts = {language: sum(status == "open" for status in registry.values())
              for language, registry in registries.items()}
    return errors, counts


def main():
    errors, counts = check(ROOT)
    for error in errors:
        print(error)
    print("open pending items: " + ", ".join(f"{key}={value}" for key, value in counts.items()))
    return bool(errors)


if __name__ == "__main__":
    sys.exit(main())
