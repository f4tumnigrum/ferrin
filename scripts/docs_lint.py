#!/usr/bin/env python3
"""Docs lint: relative links resolve, and every 【待验证】 marker outside the
appendix has a matching PV entry in docs/05-appendix/02-pending-verification.md."""
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
APPENDIX = os.path.join(ROOT, "docs", "05-appendix", "02-pending-verification.md")
LINK = re.compile(r"\[[^\]]*\]\(([^)#\s]+)(#[^)]*)?\)")

def md_files():
    for base, _, files in os.walk(ROOT):
        if "/target" in base or "/.git" in base or "/node_modules" in base:
            continue
        for name in files:
            if name.endswith(".md"):
                yield os.path.join(base, name)

def main() -> int:
    errors = []
    for path in md_files():
        text = open(path, encoding="utf-8").read()
        for match in LINK.finditer(text):
            target = match.group(1)
            if re.match(r"^[a-z]+:", target):
                continue
            resolved = os.path.normpath(os.path.join(os.path.dirname(path), target))
            if not os.path.exists(resolved):
                errors.append(f"{os.path.relpath(path, ROOT)}: broken link {target}")
    appendix = open(APPENDIX, encoding="utf-8").read()
    open_items = re.findall(r"^\| (PV-\d{3}) \|.*\| open \|$", appendix, flags=re.M)
    for path in md_files():
        if os.path.abspath(path) == APPENDIX:
            continue
        rel = os.path.relpath(path, ROOT)
        if rel == "README.md":
            continue
        for lineno, line in enumerate(open(path, encoding="utf-8"), 1):
            # A pending item is a bullet, paragraph or table cell that starts with the
            # marker; sentences that merely mention the label are not items.
            is_item = re.match(r"^\s*(?:[-*]\s+|\d+\.\s+|\|\s*)?【待验证】", line)
            if is_item and not re.search(r"PV-\d{3}", line):
                errors.append(f"{rel}:{lineno}: 【待验证】 item without PV-xxx reference")
    for error in errors:
        print(error)
    print(f"open pending items: {len(open_items)}")
    return 1 if errors else 0

if __name__ == "__main__":
    sys.exit(main())
