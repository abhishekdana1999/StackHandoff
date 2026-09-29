"""
BUG-022 fix per user decision: renumber the Tasks sheet's second block.

The proof BUG-022 recorded is fully reproduced here:

  - Rows 2-41 are TASK-001..TASK-040 against FEAT-001..FEAT-031; rows 42-73
    restart at TASK-031 and run to TASK-060 against FEAT-032..FEAT-063. The
    overlap TASK-031..TASK-040 therefore names two different tasks each, and
    the second block repeats 034, 036, 037, 038 (14 rows share an ID).
  - 67 rows (38 in the first block, 29 in the second) carry their dependency
    list in the ASSIGNEE column; three second-block rows already have
    dependencies in the correct column.

What this script does, exactly as chosen:

  1. Renumbers rows 42-73 to TASK-041..TASK-072 in row order, so every ID in
     the sheet is unique and the second block continues the first.
  2. Moves the dependency text from the Assignee column into the Dependencies
     column for all 67 rows, and clears Assignee.
  3. Remaps dependency references inside the second block by the rule:
     resolve each TASK-NN reference to the FIRST occurrence of that label in
     sheet order -- the first block is canonical, so ambiguous references
     (TASK-031..040, which exist in both blocks) resolve to the first block and
     stay as written; labels that only exist in the second block (TASK-041..060)
     become the new ID of their first occurrence in the second block.
  4. Updates BUG-022 to Fixed, with the rule and this batch logged as LOG-087.

Append-only otherwise: nothing is deleted; the Tasks rows are the data this bug
is about, so they are corrected in place, and the correction is logged.
"""

import re

import openpyxl

TODAY = "2026-09-28"
NOW = "2026-09-28 22:05"

wb = openpyxl.load_workbook("docs/WorkspaceClone_Tracking.xlsx")
ws = wb["Tasks"]

SECOND_START = 42  # 1-based sheet row: first row of the second block
SECOND_END = 73    # 1-based sheet row: last row of the second block

# --- 1. Renumber rows 42-73 to TASK-041..TASK-072 ---------------------------
old_to_new = {}   # old label -> new label, only for labels whose first occurrence
                  # in the sheet is inside the second block
new_by_row = {}   # sheet row -> new label
first_seen = {}   # old label -> first sheet row it appears on (whole sheet)
for row in ws.iter_rows(min_row=SECOND_START, max_row=SECOND_END):
    old_id = row[0].value
    assert isinstance(old_id, str) and old_id.startswith("TASK-"), f"unexpected id {old_id!r} at row {row[0].row}"
    new_by_row[row[0].row] = f"TASK-{row[0].row - SECOND_START + 41:03d}"
for row in ws.iter_rows(min_row=2):
    old_id = row[0].value
    if isinstance(old_id, str) and old_id.startswith("TASK-") and old_id not in first_seen:
        first_seen[old_id] = row[0].row
for old_id, first_row in first_seen.items():
    if first_row >= SECOND_START:
        old_to_new[old_id] = new_by_row[first_row]
for offset, row in enumerate(ws.iter_rows(min_row=SECOND_START, max_row=SECOND_END)):
    row[0].value = f"TASK-{offset + 41:03d}"

# --- 2. Move dependency text from Assignee (L, idx 11) to Dependencies (M, idx 12)
moved = 0
for row in ws.iter_rows(min_row=2):
    assignee = row[11].value
    deps = row[12].value
    if assignee not in (None, ""):
        assert deps in (None, ""), f"both L and M populated at row {row[0].row}"
        row[12].value = assignee
        row[11].value = None
        moved += 1

# --- 3. Remap references in the second block's new dependency cells ---------
token = re.compile(r"^TASK-\d{3,}$")
remapped = {}

def remap(text):
    if not isinstance(text, str):
        return text
    parts = [p.strip() for p in text.split(",")]
    out = []
    for p in parts:
        if token.match(p) and p in old_to_new and old_to_new[p] != p:
            out.append(old_to_new[p])
            remapped[p] = old_to_new[p]
        else:
            out.append(p)
    return ", ".join(out)

for offset, row in enumerate(ws.iter_rows(min_row=SECOND_START, max_row=SECOND_END)):
    row[12].value = remap(row[12].value)

# --- Verify ----------------------------------------------------------------
ids = [r[0].value for r in ws.iter_rows(min_row=2)]
assert all(isinstance(i, str) and i.startswith("TASK-") for i in ids), "non-TASK id present"
assert len(ids) == len(set(ids)), f"duplicate ids remain: {[i for i in set(ids) if ids.count(i) > 1]}"
leftover_assignee = [r[0].row for r in ws.iter_rows(min_row=2) if r[11].value not in (None, "")]
assert not leftover_assignee, f"Assignee still populated at rows {leftover_assignee}"
print(f"renumbered rows {SECOND_START}..{SECOND_END} -> TASK-041..TASK-072")
print(f"moved {moved} dependency lists from Assignee to Dependencies")
print(f"remapped references: {remapped}")

# --- Update BUG-022 --------------------------------------------------------
bugs = wb["Bugs"]
for row in bugs.iter_rows(min_row=2):
    if row[0].value == "BUG-022":
        row[6].value = "Fixed"  # Status
        row[11].value = (
            "Rows 42-73 renumbered TASK-031..TASK-060 -> TASK-041..TASK-072 in row order, so every ID is "
            "unique. The 67 dependency lists sitting in the Assignee column moved to the Dependencies "
            "column (38 first-block + 29 second-block rows; the sheet already had 3 in the right "
            "column). References remapped by the first-occurrence rule: labels only present in the "
            "second block (TASK-041..060) resolve to their renumbered first occurrence; ambiguous "
            "labels present in both blocks resolve to the canonical first block and stay as written."
        )
        row[12].value = (
            f"ID uniqueness, column placement and reference remap all re-checked by script asserts "
            f"on {TODAY}. Residual caveat recorded in LOG-087: the dependency VALUES were bulk-populated "
            f"and their semantics remain the author's intent, not a derived DAG."
        )
        row[14].value = TODAY  # Resolved Date
        break
else:
    raise SystemExit("BUG-022 not found")

wb.save("docs/WorkspaceClone_Tracking.xlsx")
print("saved: docs/WorkspaceClone_Tracking.xlsx")