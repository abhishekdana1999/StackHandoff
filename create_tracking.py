import openpyxl
from openpyxl.styles import Font, PatternFill, Alignment, Border, Side
from openpyxl.utils import get_column_letter
from datetime import datetime

wb = openpyxl.Workbook()

# Styles
header_font = Font(bold=True, color="FFFFFF", size=11)
header_fill = PatternFill(start_color="2F5496", end_color="2F5496", fill_type="solid")
subheader_fill = PatternFill(start_color="D6E4F0", end_color="D6E4F0", fill_type="solid")
subheader_font = Font(bold=True, size=11)
normal_font = Font(size=10)
thin_border = Border(
    left=Side(style='thin'),
    right=Side(style='thin'),
    top=Side(style='thin'),
    bottom=Side(style='thin')
)
status_colors = {
    "Not Started": PatternFill(start_color="FFC7CE", end_color="FFC7CE", fill_type="solid"),
    "In Progress": PatternFill(start_color="FFEB9C", end_color="FFEB9C", fill_type="solid"),
    "Running": PatternFill(start_color="C6EFCE", end_color="C6EFCE", fill_type="solid"),
    "Done": PatternFill(start_color="C6EFCE", end_color="C6EFCE", fill_type="solid"),
    "Blocked": PatternFill(start_color="FFC7CE", end_color="FFC7CE", fill_type="solid"),
    "Error": PatternFill(start_color="FF0000", end_color="FF0000", fill_type="solid"),
    "Fixed": PatternFill(start_color="C6EFCE", end_color="C6EFCE", fill_type="solid"),
}

def style_header(ws, row, max_col):
    for col in range(1, max_col + 1):
        cell = ws.cell(row=row, column=col)
        cell.font = header_font
        cell.fill = header_fill
        cell.alignment = Alignment(horizontal='center', vertical='center', wrap_text=True)
        cell.border = thin_border

def style_data(ws, start_row, end_row, max_col):
    for row in range(start_row, end_row + 1):
        for col in range(1, max_col + 1):
            cell = ws.cell(row=row, column=col)
            cell.font = normal_font
            cell.alignment = Alignment(vertical='top', wrap_text=True)
            cell.border = thin_border

def auto_width(ws, max_col, min_width=12, max_width=50):
    for col in range(1, max_col + 1):
        max_len = min_width
        for row in ws.iter_rows(min_col=col, max_col=col, values_only=False):
            for cell in row:
                if cell.value:
                    max_len = max(max_len, min(len(str(cell.value)), max_width))
        ws.column_dimensions[get_column_letter(col)].width = max_len + 2

# ============================================================
# SHEET 1: OVERVIEW
# ============================================================
ws = wb.active
ws.title = "Overview"

overview_headers = ["Field", "Value", "Status", "Last Updated", "Notes"]
overview_data = [
    ["Project Name", "Workspace Clone", "Active", datetime.now().strftime("%Y-%m-%d %H:%M"), "Cross-device work-session continuity"],
    ["Product Category", "Developer Tools / Desktop App", "Active", datetime.now().strftime("%Y-%m-%d %H:%M"), ""],
    ["Target Platforms", "Windows 11, macOS 13+ (Linux post-MVP)", "Planned", datetime.now().strftime("%Y-%m-%d %H:%M"), ""],
    ["Tech Stack", "Tauri 2 + React + TypeScript (UI), Rust (Core)", "Decided", datetime.now().strftime("%Y-%m-%d %H:%M"), "Per blueprint recommendation"],
    ["Database", "SQLite (local persistence)", "Decided", datetime.now().strftime("%Y-%m-%d %H:%M"), "Encrypted sensitive fields"],
    ["Protocol", "Noise/TLS 1.3 + mDNS discovery", "Decided", datetime.now().strftime("%Y-%m-%d %H:%M"), "End-to-end encrypted"],
    ["Key Storage", "OS Secure Storage (DPAPI/Keychain)", "Decided", datetime.now().strftime("%Y-%m-%d %H:%M"), "Non-exportable keys"],
    ["Current Phase", "Phase 0 - Validate Interactions", "In Progress", datetime.now().strftime("%Y-%m-%d %H:%M"), "Prototype two-machine journey"],
    ["MVP Scope", "Pairing, Capture, Preflight, Restore, LAN Transfer", "Planned", datetime.now().strftime("%Y-%m-%d %H:%M"), ""],
    ["Repository", "TBD", "Not Started", datetime.now().strftime("%Y-%m-%d %H:%M"), ""],
    ["Security Review", "Required before Public Beta", "Not Started", datetime.now().strftime("%Y-%m-%d %H:%M"), ""],
]

for i, header in enumerate(overview_headers, 1):
    ws.cell(row=1, column=i, value=header)
style_header(ws, 1, len(overview_headers))

for row_idx, row_data in enumerate(overview_data, 2):
    for col_idx, value in enumerate(row_data, 1):
        cell = ws.cell(row=row_idx, column=col_idx, value=value)
        if col_idx == 3 and value in status_colors:
            cell.fill = status_colors[value]
style_data(ws, 2, len(overview_data) + 1, len(overview_headers))
auto_width(ws, len(overview_headers))

# ============================================================
# SHEET 2: EPICS
# ============================================================
ws2 = wb.create_sheet("Epics")
epic_headers = ["Epic ID", "Epic Name", "Description", "Phase", "Priority", "Status", "Progress %", "Start Date", "Target Date", "Owner", "Dependencies", "Notes"]
epic_data = [
    ["EPIC-001", "Project Foundation & Setup", "Initialize Tauri 2 + React + TypeScript + Rust workspace, CI/CD, linting, testing infrastructure", "Phase 1", "P0", "Not Started", 0, "", "", "", "", "Foundation for all other work"],
    ["EPIC-002", "Local Database & Schema", "SQLite schema for devices, workspaces, snapshots, restore runs, adapter checks. Encryption for sensitive fields.", "Phase 1", "P0", "Not Started", 0, "", "", "", "EPIC-001", ""],
    ["EPIC-003", "Workspace Manifest Model", "TypeScript/Rust types, validation, serialization, schema versioning, secret scrubbing, capture preview", "Phase 1", "P0", "Not Started", 0, "", "", "", "EPIC-001", ""],
    ["EPIC-004", "Capture Pipeline & Adapters (Core)", "Git, VS Code, Browser URLs, Terminal, Runtime/CLI detection adapters. Capture selection UI.", "Phase 1", "P0", "Not Started", 0, "", "", "", "EPIC-003", ""],
    ["EPIC-005", "Device Pairing & Trust", "Key generation, OS secure storage, pairing ceremony (QR/code), verification, trust scopes, revocation", "Phase 2", "P0", "Not Started", 0, "", "", "", "EPIC-001, EPIC-002", "Critical security component"],
    ["EPIC-006", "LAN Discovery & Encrypted Transfer", "mDNS/DNS-SD discovery, Noise/TLS authenticated channel, manifest transfer with retry/cancel", "Phase 2", "P0", "Not Started", 0, "", "", "", "EPIC-005", ""],
    ["EPIC-007", "Preflight Engine & Identity Readiness", "Check categories: apps, identity, toolchain, repo, env, services, permissions. Readiness states.", "Phase 3", "P0", "Not Started", 0, "", "", "", "EPIC-002, EPIC-003", "Core differentiator"],
    ["EPIC-008", "Restore Planning & Execution", "Plan generation, dependency ordering, user approval, safe actions (open, launch, command recipes), report", "Phase 3", "P0", "Not Started", 0, "", "", "", "EPIC-007", ""],
    ["EPIC-009", "UI/UX - All Screens", "Welcome, Devices, Workspaces, Capture, Transfer, Preflight, Prepare, Restore Preview, Report, Settings", "Phase 1-3", "P1", "Not Started", 0, "", "", "", "EPIC-001", "Iterate with user testing"],
    ["EPIC-010", "Cross-Platform Path Mapping", "Named destination roots, OS-specific path translation, project location rules", "Phase 3", "P1", "Not Started", 0, "", "", "", "EPIC-003", ""],
    ["EPIC-011", "Security Hardening & Review", "Threat model validation, penetration testing, signed releases, crash resilience, diagnostics", "Phase 4", "P0", "Not Started", 0, "", "", "", "All prior", "Independent review required"],
    ["EPIC-012", "Post-MVP: Linux, Relay, Team Features", "Linux support, optional encrypted relay, team manifests, policy, additional adapters", "Phase 5", "P2", "Not Started", 0, "", "", "", "EPIC-011", ""],
]

for i, header in enumerate(epic_headers, 1):
    ws2.cell(row=1, column=i, value=header)
style_header(ws2, 1, len(epic_headers))

for row_idx, row_data in enumerate(epic_data, 2):
    for col_idx, value in enumerate(row_data, 1):
        cell = ws2.cell(row=row_idx, column=col_idx, value=value)
        if col_idx == 6 and value in status_colors:
            cell.fill = status_colors[value]
style_data(ws2, 2, len(epic_data) + 1, len(epic_headers))
auto_width(ws2, len(epic_headers))

# ============================================================
# SHEET 3: FEATURES
# ============================================================
ws3 = wb.create_sheet("Features")
feat_headers = ["Feature ID", "Epic ID", "Feature Name", "Description", "Component", "Priority", "Status", "Progress %", "Est. Days", "Actual Days", "Start Date", "End Date", "Assignee", "Dependencies", "Acceptance Criteria", "Notes"]
feat_data = [
    # EPIC-001 Features
    ["FEAT-001", "EPIC-001", "Tauri 2 Workspace Init", "Create Tauri 2 project with React + TypeScript + Rust workspace structure", "Core/Infra", "P0", "Not Started", 0, 2, 0, "", "", "", "", "Builds successfully, dev server runs", ""],
    ["FEAT-002", "EPIC-001", "CI/CD Pipeline", "GitHub Actions: lint, typecheck, test, build (Windows/macOS), release signing", "Core/Infra", "P0", "Not Started", 0, 3, 0, "", "", "", "FEAT-001", "All checks pass on PR", ""],
    ["FEAT-003", "EPIC-001", "Development Tooling", "ESLint, Prettier, Rustfmt, Clippy, pre-commit hooks, VS Code settings", "Core/Infra", "P1", "Not Started", 0, 1, 0, "", "", "", "FEAT-001", "Consistent code style", ""],
    ["FEAT-004", "EPIC-001", "Testing Infrastructure", "Vitest (unit), Playwright (e2e), Rust unit/integration tests, test utilities", "Core/Infra", "P1", "Not Started", 0, 2, 0, "", "", "", "FEAT-001", "Coverage > 80% critical paths", ""],
    
    # EPIC-002 Features
    ["FEAT-005", "EPIC-002", "SQLite Schema & Migrations", "Devices, Workspaces, Snapshots, RestoreRuns, AdapterChecks tables + migrations", "Core/DB", "P0", "Not Started", 0, 2, 0, "", "", "", "FEAT-001", "Schema matches blueprint §15", ""],
    ["FEAT-006", "EPIC-002", "Database Encryption", "Encrypt sensitive fields (device keys, workspace metadata) using SQLCipher or app-level", "Core/DB", "P0", "Not Started", 0, 2, 0, "", "", "", "FEAT-005", "Keys in OS secure storage", ""],
    ["FEAT-007", "EPIC-002", "Repository Pattern (Rust)", "Type-safe DB access layer with connection pooling, transactions", "Core/DB", "P0", "Not Started", 0, 2, 0, "", "", "", "FEAT-005", "sqlx or rusqlite", ""],
    ["FEAT-008", "EPIC-002", "Tauri Commands for DB", "Expose DB operations to frontend via Tauri commands", "Core/DB", "P0", "Not Started", 0, 1, 0, "", "", "", "FEAT-007", ""],
    
    # EPIC-003 Features
    ["FEAT-009", "EPIC-003", "Manifest Type Definitions", "TypeScript + Rust shared types for workspace manifest (schema v1)", "Core/Model", "P0", "Not Started", 0, 2, 0, "", "", "", "FEAT-001", "Matches blueprint §7 example", ""],
    ["FEAT-010", "EPIC-003", "Schema Validation", "JSON Schema + runtime validation (zod/serde), version migration", "Core/Model", "P0", "Not Started", 0, 2, 0, "", "", "", "FEAT-009", "Reject unknown critical fields", ""],
    ["FEAT-011", "EPIC-003", "Secret Scrubber", "Redact credentials from URLs, scan for secret patterns, reject secret-bearing fields", "Core/Model", "P0", "Not Started", 0, 3, 0, "", "", "", "FEAT-009", "Defense in depth per §8.5", ""],
    ["FEAT-012", "EPIC-003", "Capture Preview UI", "Show included/excluded fields, toggles for sensitive hints before transfer", "UI/Capture", "P0", "Not Started", 0, 2, 0, "", "", "", "FEAT-011", "Per blueprint §8.6", ""],
    ["FEAT-013", "EPIC-003", "Manifest Versioning & Snapshots", "Local snapshots, capture timestamps, no silent overwrites", "Core/Model", "P1", "Not Started", 0, 1, 0, "", "", "", "FEAT-009", "Per blueprint §8.7", ""],
    
    # EPIC-004 Features
    ["FEAT-014", "EPIC-004", "Git Adapter", "Detect repo root, branch, remote, dirty status, commit hash. No credential access.", "Adapters/Git", "P0", "Not Started", 0, 3, 0, "", "", "", "FEAT-009", "Per blueprint §11", ""],
    ["FEAT-015", "EPIC-004", "VS Code Adapter", "Detect install, workspace file/folder, optional extension list (opt-in)", "Adapters/VSCode", "P0", "Not Started", 0, 3, 0, "", "", "", "FEAT-009", "No settings/auth copy", ""],
    ["FEAT-016", "EPIC-004", "Browser URL Adapter", "Capture selected URLs (extension or manual), open URLs in target browser", "Adapters/Browser", "P0", "Not Started", 0, 3, 0, "", "", "", "FEAT-009", "No profile scraping", ""],
    ["FEAT-017", "EPIC-004", "Terminal Adapter", "Detect terminal, cwd, launch recipes (no scrollback/session capture)", "Adapters/Terminal", "P0", "Not Started", 0, 2, 0, "", "", "", "FEAT-009", "Commands inert until approved", ""],
    ["FEAT-018", "EPIC-004", "Runtime/CLI Adapter", "Detect Node, pnpm, Git, Docker, provider CLIs versions via bounded commands", "Adapters/Runtime", "P0", "Not Started", 0, 2, 0, "", "", "", "FEAT-009", "Sanitize outputs", ""],
    ["FEAT-019", "EPIC-004", "Capture Selection UI", "Select projects, apps, URLs, optional clipboard/files. Preview before capture.", "UI/Capture", "P0", "Not Started", 0, 3, 0, "", "", "", "FEAT-014..018", "Per blueprint §8.1", ""],
    
    # EPIC-005 Features
    ["FEAT-020", "EPIC-005", "Device Key Generation", "Ed25519 keypair, store private key in OS secure storage (non-exportable)", "Core/Crypto", "P0", "Not Started", 0, 2, 0, "", "", "", "FEAT-006", "Per blueprint §6, §17", ""],
    ["FEAT-021", "EPIC-005", "Pairing Ceremony", "QR code + human-readable code, 5-min expiry, mutual key exchange, safety number verification", "Core/Pairing", "P0", "Not Started", 0, 4, 0, "", "", "", "FEAT-020", "Per blueprint §6.1", ""],
    ["FEAT-022", "EPIC-005", "Trust Scopes & Permissions", "Receive/send workspaces, optional file transfer, optional clipboard transfer", "Core/Pairing", "P0", "Not Started", 0, 1, 0, "", "", "", "FEAT-021", "Per blueprint §6.1 step 5", ""],
    ["FEAT-023", "EPIC-005", "Device Management UI", "List paired devices, online/offline, rename, revoke, last seen, fingerprint", "UI/Devices", "P0", "Not Started", 0, 2, 0, "", "", "", "FEAT-021", "Per blueprint §14.2", ""],
    ["FEAT-024", "EPIC-005", "Revocation & Deny-List", "Local revoke, signed revocation notice, offline deny-list, re-confirm on key change", "Core/Pairing", "P0", "Not Started", 0, 2, 0, "", "", "", "FEAT-021", "Per blueprint §6.2", ""],
    ["FEAT-025", "EPIC-005", "Rate Limiting & Attempt Limits", "Limit pairing attempts, expire codes, rate-limit discovery/handshake", "Core/Pairing", "P1", "Not Started", 0, 1, 0, "", "", "", "FEAT-021", "Per blueprint §6.2", ""],
    
    # EPIC-006 Features
    ["FEAT-026", "EPIC-006", "mDNS/DNS-SD Discovery", "Advertise rotating device ID + protocol version. No trust from discovery.", "Network/Discovery", "P0", "Not Started", 0, 3, 0, "", "", "", "FEAT-020", "Per blueprint §6.3, §15", ""],
    ["FEAT-027", "EPIC-006", "Authenticated Encrypted Channel", "Noise protocol or TLS 1.3 with mutual auth. Replay protection, sequence/nonce.", "Network/Transport", "P0", "Not Started", 0, 4, 0, "", "", "", "FEAT-020", "Per blueprint §5, §15", ""],
    ["FEAT-028", "EPIC-006", "Manifest Transfer Protocol", "Chunked transfer, progress, cancel, resume after re-auth, idempotent duplicate handling", "Network/Transfer", "P0", "Not Started", 0, 3, 0, "", "", "", "FEAT-027", "Per blueprint §8.8, §15", ""],
    ["FEAT-029", "EPIC-006", "Manual Address Fallback", "Enter device address manually when LAN discovery fails", "Network/Discovery", "P1", "Not Started", 0, 1, 0, "", "", "", "FEAT-026", "Per blueprint §6.3", ""],
    ["FEAT-030", "EPIC-006", "Transfer UI", "Select paired destination, show encryption/trust status, progress, cancel", "UI/Transfer", "P0", "Not Started", 0, 2, 0, "", "", "", "FEAT-028", "Per blueprint §14.5", ""],
    
    # EPIC-007 Features
    ["FEAT-031", "EPIC-007", "Preflight Engine Core", "Run all check categories, aggregate results, compute readiness (informational only)", "Core/Preflight", "P0", "Not Started", 0, 3, 0, "", "", "", "FEAT-005, FEAT-009", "Per blueprint §9", ""],
    ["FEAT-032", "EPIC-007", "App/Tool Detection Checks", "Verify required apps installed, versions, OS compatibility", "Adapters/Preflight", "P0", "Not Started", 0, 2, 0, "", "", "", "FEAT-031", ""],
    ["FEAT-033", "EPIC-007", "Identity Readiness Adapters", "GitHub (gh auth status), AWS (sts), Supabase, etc. Confidence states.", "Adapters/Identity", "P0", "Not Started", 0, 4, 0, "", "", "", "FEAT-031", "Per blueprint §9.2-9.3", ""],
    ["FEAT-034", "EPIC-007", "Repository & File Checks", "Project root mapping, remote identity, branch state, dirty changes, expected files", "Adapters/Preflight", "P0", "Not Started", 0, 2, 0, "", "", "", "FEAT-031", ""],
    ["FEAT-035", "EPIC-007", "Environment Presence Checks", "Check named env vars exist (values never read), from safe sources only", "Adapters/Preflight", "P0", "Not Started", 0, 1, 0, "", "", "", "FEAT-031", "Per blueprint §9.5", ""],
    ["FEAT-036", "EPIC-007", "Service & Port Checks", "Local service availability (Docker, DB), port status. No network scanning.", "Adapters/Preflight", "P1", "Not Started", 0, 2, 0, "", "", "", "FEAT-031", "Per blueprint §9.6", ""],
    ["FEAT-037", "EPIC-007", "Permission Checks", "OS permissions for features (Accessibility for window layout)", "Adapters/Preflight", "P1", "Not Started", 0, 1, 0, "", "", "", "FEAT-031", "Per blueprint §9.7", ""],
    ["FEAT-038", "EPIC-007", "Preflight Report UI", "Grouped ready/missing/mismatch/unknown with evidence freshness. Rerun individual checks.", "UI/Preflight", "P0", "Not Started", 0, 3, 0, "", "", "", "FEAT-031", "Per blueprint §14.6", ""],
    ["FEAT-039", "EPIC-007", "Prepare Destination UI", "Official install links, launch provider login flows, rerun checks", "UI/Preflight", "P0", "Not Started", 0, 2, 0, "", "", "", "FEAT-038", "Per blueprint §9.8", ""],
    
    # EPIC-008 Features
    ["FEAT-040", "EPIC-008", "Restore Plan Generator", "Map source paths → destination roots, inspect Git state, dependency ordering", "Core/Restore", "P0", "Not Started", 0, 3, 0, "", "", "", "FEAT-031", "Per blueprint §10", ""],
    ["FEAT-041", "EPIC-008", "Safe Action Framework", "Allow-listed executables, typed args, constrained vars, preview, approval, timeouts", "Core/Restore", "P0", "Not Started", 0, 4, 0, "", "", "", "FEAT-040", "Per blueprint §10.2", ""],
    ["FEAT-042", "EPIC-008", "Git Safe Restore", "Never auto-stash/reset/clean. Offer open-as-is, choose path, reviewed Git action", "Adapters/Git", "P0", "Not Started", 0, 2, 0, "", "", "", "FEAT-040", "Per blueprint §10.4", ""],
    ["FEAT-043", "EPIC-008", "App Launch & URL Open Actions", "Open editor, browser URLs (domain preview, block dangerous schemes)", "Adapters/Restore", "P0", "Not Started", 0, 2, 0, "", "", "", "FEAT-040", "Per blueprint §10.2", ""],
    ["FEAT-044", "EPIC-008", "Command Recipe Execution", "Visible terminal, typed recipes, explicit approval, per-step timeout, no background", "Adapters/Terminal", "P0", "Not Started", 0, 3, 0, "", "", "", "FEAT-041", "Per blueprint §10.2", ""],
    ["FEAT-045", "EPIC-008", "File Transfer (Optional)", "Explicit, path-scoped, size-limited, symlink-safe, preview, conflict handling", "Core/Transfer", "P1", "Not Started", 0, 3, 0, "", "", "", "FEAT-041", "Post-MVP per blueprint", ""],
    ["FEAT-046", "EPIC-008", "Clipboard Restore (Opt-in)", "Off by default, preview, never silent replace", "Core/Transfer", "P2", "Not Started", 0, 1, 0, "", "", "", "FEAT-041", "Per blueprint §10.2", ""],
    ["FEAT-047", "EPIC-008", "Restore Preview UI", "Ordered actions, exact paths, commands, conflicts, approval controls", "UI/Restore", "P0", "Not Started", 0, 3, 0, "", "", "", "FEAT-040", "Per blueprint §14.8", ""],
    ["FEAT-048", "EPIC-008", "Restore Execution & Report", "Execute in order, cancellation, per-step timeout, success/skipped/failed report, retry", "UI/Restore", "P0", "Not Started", 0, 3, 0, "", "", "", "FEAT-047", "Per blueprint §14.9", ""],
    
    # EPIC-009 Features (UI Screens)
    ["FEAT-049", "EPIC-009", "Welcome / Permissions Screen", "Explain local operation, install on each device, request OS permissions on demand", "UI/Welcome", "P0", "Not Started", 0, 1, 0, "", "", "", "FEAT-001", "Per blueprint §14.1", ""],
    ["FEAT-050", "EPIC-009", "Devices Screen", "Paired devices, online/offline, trust scopes, version, revoke/rename", "UI/Devices", "P0", "Not Started", 0, 2, 0, "", "", "", "FEAT-023", "Per blueprint §14.2", ""],
    ["FEAT-051", "EPIC-009", "Workspaces Screen", "Recent captures, timestamp, source machine, readiness, version history", "UI/Workspaces", "P0", "Not Started", 0, 2, 0, "", "", "", "FEAT-013", "Per blueprint §14.3", ""],
    ["FEAT-052", "EPIC-009", "Capture Screen", "Select project/app context, inspect included/excluded data", "UI/Capture", "P0", "Not Started", 0, 2, 0, "", "", "", "FEAT-019", "Per blueprint §14.4", ""],
    ["FEAT-053", "EPIC-009", "Transfer Screen", "Select destination, encryption status, progress", "UI/Transfer", "P0", "Not Started", 0, 1, 0, "", "", "", "FEAT-030", "Per blueprint §14.5", ""],
    ["FEAT-054", "EPIC-009", "Preflight Screen", "Grouped checks with evidence, rerun, prepare actions", "UI/Preflight", "P0", "Not Started", 0, 2, 0, "", "", "", "FEAT-038", "Per blueprint §14.6", ""],
    ["FEAT-055", "EPIC-009", "Prepare Screen", "Install links, login buttons, rerun checks", "UI/Preflight", "P0", "Not Started", 0, 1, 0, "", "", "", "FEAT-039", "Per blueprint §14.7", ""],
    ["FEAT-056", "EPIC-009", "Restore Preview Screen", "Ordered actions, paths, commands, conflicts, approvals", "UI/Restore", "P0", "Not Started", 0, 2, 0, "", "", "", "FEAT-047", "Per blueprint §14.8", ""],
    ["FEAT-057", "EPIC-009", "Restore Report Screen", "Success/skipped/failed, actions taken, retry options", "UI/Restore", "P0", "Not Started", 0, 1, 0, "", "", "", "FEAT-048", "Per blueprint §14.9", ""],
    ["FEAT-058", "EPIC-009", "Settings / Privacy Screen", "Capture defaults, clipboard policy, retention, permissions, network, diagnostics, revocation", "UI/Settings", "P1", "Not Started", 0, 2, 0, "", "", "", "FEAT-001", "Per blueprint §14.10", ""],
    
    # EPIC-010 Features
    ["FEAT-059", "EPIC-010", "Named Destination Roots", "Configure per-device project root mappings (e.g., ~/projects, ~/code)", "Core/PathMap", "P1", "Not Started", 0, 2, 0, "", "", "", "FEAT-005", "Per blueprint §10.3", ""],
    ["FEAT-060", "EPIC-010", "Cross-Platform Path Translation", "Windows ↔ macOS path mapping, home directory expansion", "Core/PathMap", "P1", "Not Started", 0, 1, 0, "", "", "", "FEAT-059", ""],
    
    # EPIC-011 Features
    ["FEAT-061", "EPIC-011", "Security Audit & Pen Testing", "Independent review of pairing, key storage, manifest parsing, command execution", "Security", "P0", "Not Started", 0, 5, 0, "", "", "", "EPIC-008 done", "Per blueprint §16, §17", ""],
    ["FEAT-062", "EPIC-011", "Signed Releases & Updates", "Code signing (Windows/macOS), auto-update with rollback protection", "Core/Infra", "P0", "Not Started", 0, 3, 0, "", "", "", "FEAT-002", "Per blueprint §5.16", ""],
    ["FEAT-063", "EPIC-011", "Crash Resilience & Diagnostics", "Graceful degradation, user-controlled diagnostics, redacted error reports", "Core/Infra", "P1", "Not Started", 0, 2, 0, "", "", "", "FEAT-001", "Per blueprint §12.4", ""],
]

for i, header in enumerate(feat_headers, 1):
    ws3.cell(row=1, column=i, value=header)
style_header(ws3, 1, len(feat_headers))

for row_idx, row_data in enumerate(feat_data, 2):
    for col_idx, value in enumerate(row_data, 1):
        cell = ws3.cell(row=row_idx, column=col_idx, value=value)
        if col_idx == 7 and value in status_colors:
            cell.fill = status_colors[value]
style_data(ws3, 2, len(feat_data) + 1, len(feat_headers))
auto_width(ws3, len(feat_headers), max_width=60)

# ============================================================
# SHEET 4: TASKS (Granular work items)
# ============================================================
ws4 = wb.create_sheet("Tasks")
task_headers = ["Task ID", "Feature ID", "Task Name", "Description", "Type", "Status", "Progress %", "Est. Hours", "Actual Hours", "Start Date", "End Date", "Assignee", "Dependencies", "Blockers", "Error/Issue", "Solution/Resolution", "Notes"]
# Add initial tasks for FEAT-001 (Tauri 2 Workspace Init)
initial_tasks = [
    ["TASK-001", "FEAT-001", "Install Tauri CLI", "Install @tauri-apps/cli and initialize project", "Setup", "Not Started", 0, 0.5, 0, "", "", "", "", "", "", ""],
    ["TASK-002", "FEAT-001", "Create Tauri 2 Project", "Run `cargo tauri init` with React + TypeScript template", "Setup", "Not Started", 0, 1, 0, "", "", "", "TASK-001", "", "", ""],
    ["TASK-003", "FEAT-001", "Configure Rust Workspace", "Set up Cargo workspace with core, adapters, crypto, network crates", "Setup", "Not Started", 0, 2, 0, "", "", "", "TASK-002", "", "", ""],
    ["TASK-004", "FEAT-001", "Configure Frontend", "Vite + React + TypeScript + Tailwind CSS setup", "Setup", "Not Started", 0, 1, 0, "", "", "", "TASK-002", "", "", ""],
    ["TASK-005", "FEAT-001", "Verify Build (Windows/macOS)", "Run `cargo tauri build` on both platforms, fix any issues", "Build", "Not Started", 0, 2, 0, "", "", "", "TASK-003, TASK-004", "", "", ""],
    ["TASK-006", "FEAT-002", "GitHub Actions Workflow", "Create CI workflow: lint, typecheck, test, build matrix", "CI/CD", "Not Started", 0, 3, 0, "", "", "", "TASK-005", "", "", ""],
    ["TASK-007", "FEAT-002", "Release Workflow", "Automated release on tag: build, sign, package, publish", "CI/CD", "Not Started", 0, 3, 0, "", "", "", "TASK-006", "", "", ""],
    ["TASK-008", "FEAT-003", "ESLint + Prettier Config", "Shared config for TS/JS, Rustfmt for Rust", "Tooling", "Not Started", 0, 1, 0, "", "", "", "TASK-001", "", "", ""],
    ["TASK-009", "FEAT-003", "Pre-commit Hooks", "Husky + lint-staged for staged files", "Tooling", "Not Started", 0, 0.5, 0, "", "", "", "TASK-008", "", "", ""],
    ["TASK-010", "FEAT-004", "Vitest Setup", "Unit test config, React Testing Library, coverage thresholds", "Testing", "Not Started", 0, 1, 0, "", "", "", "TASK-004", "", "", ""],
    ["TASK-011", "FEAT-004", "Playwright Setup", "E2E test config for Tauri app", "Testing", "Not Started", 0, 1, 0, "", "", "", "TASK-010", "", "", ""],
    ["TASK-012", "FEAT-004", "Rust Test Setup", "cargo test with testcontainers or mock for integration", "Testing", "Not Started", 0, 1, 0, "", "", "", "TASK-003", "", "", ""],
]

for i, header in enumerate(task_headers, 1):
    ws4.cell(row=1, column=i, value=header)
style_header(ws4, 1, len(task_headers))

for row_idx, row_data in enumerate(initial_tasks, 2):
    for col_idx, value in enumerate(row_data, 1):
        cell = ws4.cell(row=row_idx, column=col_idx, value=value)
        if col_idx == 6 and value in status_colors:
            cell.fill = status_colors[value]
style_data(ws4, 2, len(initial_tasks) + 1, len(task_headers))
auto_width(ws4, len(task_headers), max_width=60)

# ============================================================
# SHEET 5: BUGS
# ============================================================
ws5 = wb.create_sheet("Bugs")
bug_headers = ["Bug ID", "Feature/Task ID", "Title", "Description", "Severity", "Priority", "Status", "Repro Steps", "Expected", "Actual", "Root Cause", "Fix", "Verified", "Reported Date", "Resolved Date", "Assignee", "Notes"]
# Empty initially - will be populated as bugs are found
for i, header in enumerate(bug_headers, 1):
    ws5.cell(row=1, column=i, value=header)
style_header(ws5, 1, len(bug_headers))
auto_width(ws5, len(bug_headers), max_width=60)

# ============================================================
# SHEET 6: COMPONENTS
# ============================================================
ws6 = wb.create_sheet("Components")
comp_headers = ["Component ID", "Component Name", "Type", "Language", "Description", "Status", "Version", "Location", "Dependencies", "Owner", "Health", "Last Updated", "Notes"]
comp_data = [
    ["COMP-001", "Tauri App Shell", "Frontend Shell", "TypeScript/React", "Main window, IPC bridge, system tray", "Not Started", "0.0.0", "src-tauri/, src/", "Tauri 2, React 18", "", "Unknown", datetime.now().strftime("%Y-%m-%d"), ""],
    ["COMP-002", "Core Rust Crate", "Library", "Rust", "Shared types, manifest model, validation, scrubber", "Not Started", "0.0.0", "src-tauri/core/", "serde, schemars, regex", "", "Unknown", datetime.now().strftime("%Y-%m-%d"), ""],
    ["COMP-003", "Crypto Crate", "Library", "Rust", "Key generation, Noise protocol, encryption/decryption", "Not Started", "0.0.0", "src-tauri/crypto/", "snow, x25519-dalek, aes-gcm", "", "Unknown", datetime.now().strftime("%Y-%m-%d"), "Critical security"],
    ["COMP-004", "Database Crate", "Library", "Rust", "SQLite access, repositories, migrations, encryption", "Not Started", "0.0.0", "src-tauri/db/", "sqlx, sqlcipher", "", "Unknown", datetime.now().strftime("%Y-%m-%d"), ""],
    ["COMP-005", "Network Crate", "Library", "Rust", "mDNS discovery, authenticated transport, transfer protocol", "Not Started", "0.0.0", "src-tauri/network/", "mdns-sd, tokio, bytes", "", "Unknown", datetime.now().strftime("%Y-%m-%d"), ""],
    ["COMP-006", "Adapters Crate", "Library", "Rust", "Adapter trait + Git, VS Code, Browser, Terminal, Runtime implementations", "Not Started", "0.0.0", "src-tauri/adapters/", "Core, DB", "", "Unknown", datetime.now().strftime("%Y-%m-%d"), ""],
    ["COMP-007", "Preflight Crate", "Library", "Rust", "Check orchestration, identity adapters, readiness aggregation", "Not Started", "0.0.0", "src-tauri/preflight/", "Adapters, Core", "", "Unknown", datetime.now().strftime("%Y-%m-%d"), ""],
    ["COMP-008", "Restore Crate", "Library", "Rust", "Plan generation, safe action execution, Git safety, file transfer", "Not Started", "0.0.0", "src-tauri/restore/", "Adapters, Core", "", "Unknown", datetime.now().strftime("%Y-%m-%d"), ""],
    ["COMP-009", "UI - Components", "Frontend", "TypeScript/React", "Reusable UI components (buttons, forms, tables, modals)", "Not Started", "0.0.0", "src/components/", "Tailwind, Radix UI", "", "Unknown", datetime.now().strftime("%Y-%m-%d"), ""],
    ["COMP-010", "UI - Screens", "Frontend", "TypeScript/React", "Page-level screens per blueprint §14", "Not Started", "0.0.0", "src/screens/", "Components, Router", "", "Unknown", datetime.now().strftime("%Y-%m-%d"), ""],
    ["COMP-011", "UI - State Management", "Frontend", "TypeScript", "Zustand/Redux for app state, React Query for server state", "Not Started", "0.0.0", "src/store/", "Zustand, TanStack Query", "", "Unknown", datetime.now().strftime("%Y-%m-%d"), ""],
    ["COMP-012", "Tauri Commands", "Bridge", "Rust/TS", "Command definitions for all Rust → Frontend operations", "Not Started", "0.0.0", "src-tauri/commands/", "All crates", "", "Unknown", datetime.now().strftime("%Y-%m-%d"), ""],
]

for i, header in enumerate(comp_headers, 1):
    ws6.cell(row=1, column=i, value=header)
style_header(ws6, 1, len(comp_headers))

for row_idx, row_data in enumerate(comp_data, 2):
    for col_idx, value in enumerate(row_data, 1):
        cell = ws6.cell(row=row_idx, column=col_idx, value=value)
        if col_idx == 6 and value in status_colors:
            cell.fill = status_colors[value]
style_data(ws6, 2, len(comp_data) + 1, len(comp_headers))
auto_width(ws6, len(comp_headers), max_width=60)

# ============================================================
# SHEET 7: RUNS (Execution / Test Runs)
# ============================================================
ws7 = wb.create_sheet("Runs")
run_headers = ["Run ID", "Type", "Component/Feature", "Command", "Status", "Start Time", "End Time", "Duration (s)", "Exit Code", "Output Summary", "Errors", "Artifacts", "Triggered By", "Notes"]
# Empty initially
for i, header in enumerate(run_headers, 1):
    ws7.cell(row=1, column=i, value=header)
style_header(ws7, 1, len(run_headers))
auto_width(ws7, len(run_headers), max_width=60)

# ============================================================
# SHEET 8: ERRORS
# ============================================================
ws8 = wb.create_sheet("Errors")
err_headers = ["Error ID", "Date/Time", "Component", "Feature/Task", "Error Type", "Error Message", "Stack Trace", "Context", "Severity", "Status", "Root Cause", "Solution", "Fix Applied", "Verified", "Prevention", "Related Bug ID", "Notes"]
# Empty initially
for i, header in enumerate(err_headers, 1):
    ws8.cell(row=1, column=i, value=header)
style_header(ws8, 1, len(err_headers))
auto_width(ws8, len(err_headers), max_width=60)

# ============================================================
# SHEET 9: DECISIONS
# ============================================================
ws9 = wb.create_sheet("Decisions")
dec_headers = ["Decision ID", "Date", "Title", "Context", "Options Considered", "Decision", "Rationale", "Impact", "Revisit Trigger", "Owner", "Status", "Related Items"]
dec_data = [
    ["DEC-001", datetime.now().strftime("%Y-%m-%d"), "Desktop Framework: Tauri 2 vs Electron vs Flutter", "Need cross-platform desktop app with native performance and Rust backend", "Tauri 2, Electron, Flutter, Native (Swift/WinUI)", "Tauri 2", "Smaller bundle, Rust backend, web frontend, active community, matches blueprint recommendation", "All native code in Rust; web tech for UI", "If Tauri 2 has blocking issues", "Team", "Decided", "Blueprint §5"],
    ["DEC-002", datetime.now().strftime("%Y-%m-%d"), "Crypto Protocol: Noise vs TLS 1.3", "Authenticated encryption for device-to-device transfer", "Noise IK, TLS 1.3 mTLS, Custom", "Noise Protocol (snow crate)", "Designed for peer-to-peer, minimal handshake, forward secrecy, well-reviewed implementations", "Rust crypto crate design", "If interop with non-Rust needed", "Team", "Decided", "Blueprint §5, §17"],
    ["DEC-003", datetime.now().strftime("%Y-%m-%d"), "Database: SQLite vs SQLCipher vs Redwood", "Local persistence with encryption for sensitive fields", "SQLite + app-level encryption, SQLCipher, Redwood", "SQLite with SQLCipher", "Transparent encryption, mature, zero-config, OS key storage for master key", "DB crate design", "If performance issues", "Team", "Decided", "Blueprint §5, §15"],
    ["DEC-004", datetime.now().strftime("%Y-%m-%d"), "Discovery: mDNS/DNS-SD vs Custom", "LAN device discovery for pairing and transfer", "mDNS/DNS-SD, Custom UDP broadcast, Central relay", "mDNS/DNS-SD (mdns-sd crate)", "Standard, works cross-platform, rotating IDs for privacy", "Network crate design", "If enterprise networks block mDNS", "Team", "Decided", "Blueprint §6.3, §15"],
    ["DEC-005", datetime.now().strftime("%Y-%m-%d"), "State Management: Zustand vs Redux vs Context", "Frontend global state for devices, workspaces, UI", "Zustand, Redux Toolkit, React Context + useReducer", "Zustand", "Simple, TypeScript-first, minimal boilerplate, good devtools", "UI architecture", "If complex server state needs", "Team", "Decided", ""],
    ["DEC-006", datetime.now().strftime("%Y-%m-%d"), "Styling: Tailwind CSS vs CSS Modules vs Styled Components", "Consistent, maintainable styling system", "Tailwind CSS, CSS Modules, Styled Components", "Tailwind CSS", "Utility-first, fast iteration, small bundle, great TypeScript support", "UI components", "If design system needed", "Team", "Decided", ""],
    ["DEC-007", datetime.now().strftime("%Y-%m-%d"), "Manifest Format: JSON vs YAML vs CBOR", "Wire format for workspace manifests", "JSON, YAML, CBOR", "JSON (with CBOR for large file transfers)", "Human-readable, universal parser support, schema validation via JSON Schema", "Core model, Network", "If binary size critical", "Team", "Decided", "Blueprint §7, §15"],
    ["DEC-008", datetime.now().strftime("%Y-%m-%d"), "Identity Checks: CLI-only vs Browser flows", "How to verify identity readiness (GitHub, AWS, etc.)", "CLI status commands only, Browser OAuth flows, Hybrid", "Hybrid (CLI primary, browser fallback)", "CLI checks are reliable and scriptable; browser for providers without CLI", "Preflight adapters", "If CLI unreliable", "Team", "Decided", "Blueprint §9.2-9.3"],
]

for i, header in enumerate(dec_headers, 1):
    ws9.cell(row=1, column=i, value=header)
style_header(ws9, 1, len(dec_headers))

for row_idx, row_data in enumerate(dec_data, 2):
    for col_idx, value in enumerate(row_data, 1):
        cell = ws9.cell(row=row_idx, column=col_idx, value=value)
        if col_idx == 11 and value in status_colors:
            cell.fill = status_colors[value]
style_data(ws9, 2, len(dec_data) + 1, len(dec_headers))
auto_width(ws9, len(dec_headers), max_width=60)

# ============================================================
# SHEET 10: LOGS (Daily/Activity Log)
# ============================================================
ws10 = wb.create_sheet("Logs")
log_headers = ["Log ID", "Date/Time", "Type", "Component", "Feature/Task", "Message", "Details", "Author", "Tags"]
log_data = [
    ["LOG-001", datetime.now().strftime("%Y-%m-%d %H:%M"), "INIT", "Project", "EPIC-001", "Project initialized from blueprint", "Created tracking Excel, set up directory structure", "System", "init,setup"],
    ["LOG-002", datetime.now().strftime("%Y-%m-%d %H:%M"), "DECISION", "Architecture", "DEC-001", "Selected Tauri 2 + React + TypeScript + Rust", "Per blueprint recommendation §5", "Team", "decision,architecture"],
    ["LOG-003", datetime.now().strftime("%Y-%m-%d %H:%M"), "DECISION", "Architecture", "DEC-002", "Selected Noise Protocol for encrypted transport", "Peer-to-peer design, forward secrecy", "Team", "decision,crypto"],
    ["LOG-004", datetime.now().strftime("%Y-%m-%d %H:%M"), "DECISION", "Architecture", "DEC-003", "Selected SQLite + SQLCipher for encrypted local DB", "Transparent encryption, OS key storage", "Team", "decision,database"],
    ["LOG-005", datetime.now().strftime("%Y-%m-%d %H:%M"), "DECISION", "Architecture", "DEC-004", "Selected mDNS/DNS-SD for LAN discovery", "Standard, cross-platform, rotating IDs", "Team", "decision,network"],
    ["LOG-006", datetime.now().strftime("%Y-%m-%d %H:%M"), "DECISION", "Frontend", "DEC-005", "Selected Zustand for state management", "Simple, TS-first, minimal boilerplate", "Team", "decision,frontend"],
    ["LOG-007", datetime.now().strftime("%Y-%m-%d %H:%M"), "DECISION", "Frontend", "DEC-006", "Selected Tailwind CSS for styling", "Utility-first, fast iteration", "Team", "decision,frontend"],
    ["LOG-008", datetime.now().strftime("%Y-%m-%d %H:%M"), "DECISION", "Data", "DEC-007", "Selected JSON for manifests, CBOR for file transfer", "Human-readable, schema validation", "Team", "decision,data"],
    ["LOG-009", datetime.now().strftime("%Y-%m-%d %H:%M"), "DECISION", "Preflight", "DEC-008", "Hybrid CLI + browser for identity checks", "CLI reliable, browser for providers without CLI", "Team", "decision,preflight"],
]

for i, header in enumerate(log_headers, 1):
    ws10.cell(row=1, column=i, value=header)
style_header(ws10, 1, len(log_headers))

for row_idx, row_data in enumerate(log_data, 2):
    for col_idx, value in enumerate(row_data, 1):
        cell = ws10.cell(row=row_idx, column=col_idx, value=value)
style_data(ws10, 2, len(log_data) + 1, len(log_headers))
auto_width(ws10, len(log_headers), max_width=60)

# Save
output_path = "/Users/abhishekdana/Documents/openshorts/WorkspaceClone_Tracking.xlsx"
wb.save(output_path)
print(f"Tracking file created: {output_path}")
print(f"Sheets: {wb.sheetnames}")
