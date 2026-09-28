#!/usr/bin/env python3
"""Replace hardcoded Tailwind palette colours with the design-system tokens.

Every one of the 84 raw-palette usages in src/ encodes a *status*, and every
status is light-mode-only: `bg-yellow-50` is a pale cream that is unreadable on
a dark canvas, and `text-yellow-800` on a dark surface is close to invisible.
They could not be fixed by adding a dark palette, because they are not tokens.

The mapping is not uniform, and two of the rules are judgement calls worth
stating:

  - Headings keep the status colour, body copy drops to `text-text-muted`. In a
    tinted callout the fill already carries the tone; the body line was
    spending a second saturated colour on a sentence.

  - `blue` is two different things. On an icon it means "we could not determine
    this", which is the neutral family's job, not the accent's. On a callout it
    means "here is information", which is the accent. Mapping all blue to one
    token would have made every "unknown" icon look like a link.

Run with --check to report without writing.
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "src"

# Ordered: the first match wins, so compound classes are handled before the
# single-token fallbacks that would otherwise match inside them.
RULES = [
    # --- callout blocks: fill, border, heading, body ---------------------
    ("bg-yellow-50 border border-yellow-200", "bg-warning-bg border border-warning-border"),
    ("bg-blue-50 border border-blue-200", "bg-primary-soft border border-primary-soft-border"),
    ("bg-green-50 border border-green-200", "bg-success-bg border border-success-border"),
    ("bg-red-50 border border-red-200", "bg-danger-bg border border-danger-border"),
    ("bg-yellow-50", "bg-warning-bg"),
    ("border-yellow-200", "border-warning-border"),
    ("bg-blue-50", "bg-primary-soft"),
    ("border-blue-200", "border-primary-soft-border"),
    ("bg-green-50", "bg-success-bg"),
    ("border-green-200", "border-success-border"),
    ("bg-red-50", "bg-danger-bg"),
    ("border-red-200", "border-danger-border"),

    # --- callout headings: keep the tone ---------------------------------
    ("text-yellow-800", "text-warning-fg"),
    ("text-blue-800", "text-primary"),
    ("text-green-800", "text-success-fg"),
    ("text-red-800", "text-danger-fg"),

    # --- callout body: legible grey, the fill already signals the tone ----
    ("text-yellow-700", "text-text-muted"),
    ("text-blue-700", "text-text-muted"),
    ("text-green-700", "text-text-muted"),
    ("text-red-700", "text-text-muted"),

    # --- status text -----------------------------------------------------
    ("text-green-600", "text-success"),
    ("text-green-500", "text-success"),
    ("text-yellow-600", "text-warning-fg"),
    ("text-yellow-500", "text-warning"),
    ("text-red-600", "text-danger"),
    ("text-red-500", "text-danger"),
    # "we could not determine this" — neutral, deliberately not the accent
    ("text-blue-500", "text-neutral"),
    # informational emphasis
    ("text-blue-600", "text-primary"),
    # "we have no reading" metadata
    ("text-gray-400", "text-text-subtle"),
    ("text-gray-500", "text-text-subtle"),
    ("text-gray-600", "text-text-muted"),
]

# Anything matching this that survives the pass is a miss.
LEFTOVER = re.compile(
    r"\b(text|bg|border|ring|from|to|via|decoration|divide|fill|stroke|shadow|outline|accent|caret)"
    r"-(red|green|blue|yellow|amber|orange|purple|indigo|violet|pink|slate|gray|zinc|neutral"
    r"|stone|sky|teal|cyan|emerald|fuchsia|rose|lime)-[0-9]{2,3}\b"
)

# `Badge.tsx` defines its own solid variants, which the rules above cannot
# reach (they are `bg-green-500 text-white ...` in a lookup table).
BADGE_RULES = [
    ("success: 'border-transparent bg-green-500 text-white hover:bg-green-500/80'",
     "success: 'border-success-border bg-success-bg text-success-fg'"),
    ("warning: 'border-transparent bg-yellow-500 text-white hover:bg-yellow-500/80'",
     "warning: 'border-warning-border bg-warning-bg text-warning-fg'"),
    ("info: 'border-transparent bg-blue-500 text-white hover:bg-blue-500/80'",
     "info: 'border-primary-soft-border bg-primary-soft text-primary'"),
]

check_only = "--check" in sys.argv
total = 0
files_changed = []

for path in sorted(SRC.rglob("*.tsx")):
    original = path.read_text()
    text = original

    for old, new in BADGE_RULES:
        text = text.replace(old, new)

    for old, new in RULES:
        text = text.replace(old, new)

    if text != original:
        total += sum(1 for a, b in zip(original.split(), text.split()) if a != b)
        files_changed.append(path.relative_to(ROOT))
        if not check_only:
            path.write_text(text)

print(f"{'would change' if check_only else 'changed'}: {total} tokens across {len(files_changed)} files")
for f in files_changed:
    print("  ", f)

leftover = []
for path in sorted(SRC.rglob("*.tsx")):
    for i, line in enumerate(path.read_text().splitlines(), 1):
        for m in LEFTOVER.finditer(line):
            leftover.append(f"{path.relative_to(ROOT)}:{i}: {m.group(0)}")

if leftover:
    print(f"\n{len(leftover)} raw palette colour(s) still present:")
    for line in leftover:
        print("  ", line)
    sys.exit(1)

print("\nno raw palette colours remain in src/")
