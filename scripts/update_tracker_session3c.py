"""Tracker addendum: LOG-087 records the BUG-022 fix."""

import datetime as dt

import openpyxl

NOW = dt.datetime(2026, 9, 28, 22, 10).strftime("%Y-%m-%d %H:%M")

wb = openpyxl.load_workbook("docs/WorkspaceClone_Tracking.xlsx")
ws = wb["Logs"]


def log(log_id, kind, component, item, message, details, tags):
    assert ws.max_column == 9
    return [log_id, NOW, kind, component, item, message, details, "Assistant", tags]


ws.append(
    log(
        "LOG-087",
        "FIX",
        "Project Data",
        "BUG-022",
        "Tasks sheet renumbered and dependency columns fixed",
        "BUG-022 fixed per the chosen option (second block renumbered): rows 42-73 became TASK-041.."
        "TASK-072 in row order, so all 72 task rows now have unique IDs (the 14 shared IDs are gone). "
        "The 67 dependency lists sitting in the Assignee column (38 first-block + 29 second-block rows) "
        "moved into the Dependencies column. References remapped by the first-occurrence rule: only "
        "labels whose first sheet occurrence is in the second block changed -- TASK-042->056, "
        "TASK-047->061, TASK-048->062, TASK-056->068; ambiguous labels TASK-031..040 resolve to the "
        "canonical first block and were left as written. Caveat: the dependency VALUES were "
        "bulk-populated when the sheet was built and their semantics remain the original author's "
        "intent, not a derived DAG -- but the column each chain sits in is now the one its header "
        "names. Re-verified by script asserts on 2026-09-28.",
        "bug,data-quality,tasks-sheet,correction",
    )
)

wb.save("docs/WorkspaceClone_Tracking.xlsx")
print("saved:", ws.max_row - 1, "log rows")