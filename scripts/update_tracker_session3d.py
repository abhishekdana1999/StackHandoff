"""Tracker addendum: BUG-028 (link-local IPv6 send failure) fixed in the
connect-address path, ERR-007 recorded, DEC-033 logged, RUN-029 recorded,
test-coverage figure updated."""

import datetime as dt

import openpyxl

NOW = dt.datetime(2026, 9, 28, 23, 10).strftime("%Y-%m-%d %H:%M")
TODAY = "2026-09-28"

wb = openpyxl.load_workbook("WorkspaceClone_Tracking.xlsx")

# ---------------------------------------------------------------------------
# Bugs
# ---------------------------------------------------------------------------
bugs = wb["Bugs"]
assert bugs.max_column == 17
bug = [
    "BUG-028",
    "FEAT-028",
    "Mac send fails with \"No route to host\" against the laptop's link-local IPv6 address",
    "First real two-machine send from the Mac to the paired Windows laptop (BISWAJITA) failed "
    "before any data moved, twice in a row: Network error: Connection failed: Could not reach "
    "fe80::f727:abc9:2280:4f3:54108: No route to host (os error 65). The laptop was listed, "
    "paired and Send was enabled, so the failure was entirely on the connect side.",
    "Critical",
    "P0",
    "Fixed",
    "On the Mac, with the laptop discovered by mDNS, paired, and allowed to receive, click Send "
    "workspace.",
    "The send starts and the transfer proceeds.",
    "The send failed with Could not reach fe80::f727:abc9:2280:4f3:54108: No route to host "
    "(os error 65) while the device was sitting on the same screen, on-network.",
    "send_inner connected only to destination.addresses.first() as a bare string. mdns-sd's "
    "get_addresses() returns IpAddrs with no zone information, and the laptop's mDNS record "
    "listed its link-local IPv6 address (fe80::/10) first. A link-local address names the link, "
    "not a destination; macOS refuses to guess which local interface the peer is on and returns "
    "ENETUNREACH before any SYN is sent. The transfer listener also bound IPv4-only "
    "(0.0.0.0), so even a correctly scoped IPv6 attempt could never have been served.",
    "Every advertised address is now a connect candidate instead of only the first. IPv4 "
    "candidates come first (mDNS peers share a link, so IPv4 almost always just works). A bare "
    "link-local IPv6 is expanded into one scoped address per local link-local interface "
    "(fe80::x%en0 scope ids, via the new if-addrs dependency) so the connect is real instead of "
    "unroutable, and a hand-typed %zone in manual pairing is trusted as given. The candidates "
    "are raced with futures select_ok under the single existing 10 s connect budget "
    "(cancel-aware as before), first success wins, and the error names the address that "
    "refused. connect_manual uses the same candidates with its 5 s budget. The transfer "
    "listener now prefers a dual-stack bind ([::] with IPV6_V6ONLY off, via socket2, which was "
    "already in the lockfile), falling back to the old 0.0.0.0 bind, so a peer connecting over "
    "its link-local IPv6 address can be served.",
    "Yes, at source. 11 new tests take the network crate from 68 to 79: IPv4 is ordered first "
    "however the record lists addresses; a link-local address is scoped to each local "
    "link-local interface; a hand-typed %zone is kept as a single candidate; a numeric zone "
    "survives; a global IPv6 passes through unscoped; duplicates and unreadable entries are "
    "dropped; a dead first candidate does not block a live one; a refusal names the address; "
    "the dual-stack listener serves an IPv4 loopback connect through either family of bind. "
    "On this Mac the listener binds [::] yet still serves 127.0.0.1. Full suite: 405 Rust "
    "tests, 0 failures, cargo build --workspace warning-free. The real two-machine retest "
    "needs the new build on BOTH laptops; the reverse (Windows->Mac) direction is still "
    "unproven on hardware.",
    TODAY,
    TODAY,
    "Assistant",
    "The reported address as copied reads fe80::f727:abc9:2280:4f3:54108, whose final group "
    "has five hex digits; std rejects it and only the kernel resolver accepts it -- the fix "
    "does not depend on the exact spelling, and unparseable entries are skipped in favour of "
    "the addresses that do parse. Laptop retest steps: git pull, rebuild + reinstall the .exe "
    "on Windows, then send Mac->Windows and Windows->Mac.",
]
bugs.append(bug)

# ---------------------------------------------------------------------------
# Errors
# ---------------------------------------------------------------------------
errors = wb["Errors"]
assert errors.max_column == 17
err = [
    "ERR-007",
    f"{TODAY} 23:10",
    "Network Crate",
    "FEAT-028",
    "Runtime Network Error",
    "Could not reach fe80::f727:abc9:2280:4f3:54108: No route to host (os error 65)",
    "No panic or backtrace; the failure surfaced through the transfer session error field, "
    "shown as 'Network error: Connection failed: ...' on the Mac UI, twice in a row.",
    "First real two-machine send from the Mac to the paired Windows laptop. mDNS advertised the "
    "laptop's link-local IPv6 address first and the sender connected only to the first "
    "advertised address as a bare string.",
    "Critical",
    "Resolved",
    "A bare IPv6 link-local (fe80::/10) needs a zone id (scope) before the kernel will route "
    "to it; macOS returns ENETUNREACH (os error 65). The receiver also listened IPv4-only "
    "(0.0.0.0), so even a scoped IPv6 connect could not have been served.",
    "Connect-candidate expansion (IPv4 first; link-local IPv6 scoped to local link-local "
    "interfaces; %zone trusted) raced under one budget, plus a dual-stack [::] listener with "
    "an IPv4-only fallback.",
    "Yes",
    "Yes - 405 Rust tests pass, including 11 new ones covering ordering, scoping, refused-"
    "address naming and the dual-stack listener; the listener binds [::] on this Mac and still "
    "serves 127.0.0.1. The two-machine retest is pending the new build on both laptops.",
    "A connect to a peer must always come from the candidate list, never from "
    "addresses.first(); link-local IPv6 is scoped before any connect.",
    "BUG-028",
    "os error 65 = ENETUNREACH. The same bare-fe80 problem could bite any future direct-IP "
    "connect path, which is why the candidate helper is shared by send and manual pairing.",
]
errors.append(err)

# ---------------------------------------------------------------------------
# Decisions
# ---------------------------------------------------------------------------
decisions = wb["Decisions"]
assert decisions.max_column == 12
dec = [
    "DEC-033",
    TODAY,
    "Connect candidates (IPv4 first, scoped link-local) and a dual-stack listener",
    "A discovered device's first advertised address can be a bare link-local IPv6 string, "
    "which macOS refuses to route (ENETUNREACH, os error 65). The receiver also listened "
    "IPv4-only, so the IPv6 path could never have been served even when scoped. The send "
    "connected only to addresses.first(), so everything depended on mDNS answer order.",
    "(a) Sort IPv4 ahead and keep connecting to the first address. (b) Try every advertised "
    "address, IPv4 first, racing them under one budget. IPv6: (c) leave link-local bare, (d) "
    "add per-interface scope via interface enumeration (if-addrs), (e) accept a hand-typed "
    "%zone. Listener: (f) keep 0.0.0.0, (g) prefer dual-stack [::] with IPV6_V6ONLY off, "
    "falling back.",
    "(b)+(d)+(e)+(g). candidate_socket_addrs turns every advertised string into candidates: "
    "IPv4 first; a bare link-local IPv6 expanded to one scoped socket per local link-local "
    "interface (index from if-addrs), with a bare fallback only when no interface can scope "
    "it; %zone trusted as given. connect_any races them with futures select_ok under the "
    "single connect budget (10 s send, 5 s manual pairing), first success wins. The transfer "
    "listener prefers a dual-stack bind ([::], V6ONLY off, socket2) and falls back to the "
    "long-standing 0.0.0.0 bind.",
    "IPv4-first alone fixes the reported case but leaves IPv6-only links producing the same "
    "confusing error; scoping a link-local address is the only way a connect to it can "
    "succeed. A dual-stack listener is required for a v6-only send to complete end to end. "
    "Racing keeps the UX at one budget (no N-candidate timeout stack) and names the refusing "
    "address, and both affected connect paths share the same helper so the rule can't "
    "drift.",
    "if-addrs and socket2 become direct dependencies of the network crate (both already in "
    "the Cargo.lock via tokio/mdns-sd). Manual pairing now accepts fe80::x%en0. Error "
    "messages name the attempted candidate. 11 new tests; suite 405.",
    "If transfers ever route over relays or tunnels where link-local addressing is "
    "irrelevant, the scoping machinery is harmless but could be simplified.",
    "Assistant",
    "Decided",
    "BUG-028,FEAT-028",
]
decisions.append(dec)

# ---------------------------------------------------------------------------
# Runs
# ---------------------------------------------------------------------------
runs = wb["Runs"]
assert runs.max_column == 14
run = [
    "RUN-029",
    "Test",
    "Network Crate / BUG-028",
    "cargo test --workspace -- --test-threads=2",
    "Passed",
    f"{TODAY} 23:00",
    f"{TODAY} 23:02",
    120,
    0,
    "405 Rust tests passed. Network crate lib rose 68 -> 79 with the 11 new connect-candidate "
    "and dual-stack listener tests; loopback_transfer 8, two_device_transfer 12, commands 66, "
    "adapters 62 with parity contract 6. No failures anywhere.",
    None,
    None,
    "Assistant",
    "Companion gate: cargo build --workspace is warning-free (the only warning anywhere is a "
    "pre-existing unused-mut in wire.rs test code, invisible to the build gate). Frontend "
    "untouched, so 99 vitest + tsc stay as RUN-026/027 state.",
]
runs.append(run)

# ---------------------------------------------------------------------------
# Logs
# ---------------------------------------------------------------------------
logs = wb["Logs"]
assert logs.max_column == 9
log_entry = [
    "LOG-088",
    NOW,
    "FIX",
    "Network Crate",
    "BUG-028,FEAT-028",
    "Send failure \"No route to host\" fixed: every advertised address is a connect candidate",
    "BUG-028 resolved in transfer.rs + discovery.rs. send_inner no longer connects to "
    "addresses.first(): candidate_socket_addrs expands every advertised string (IPv4 first; "
    "bare link-local IPv6 scoped per local link-local interface via if-addrs; hand-typed "
    "%zone trusted; dedup; unparseable entries skipped) and connect_any races them under the "
    "single 10s budget with futures select_ok, first success wins. connect_manual reuses the "
    "same helper with its 5s budget, so fe80::x%en0 works in the pairing dialog. The transfer "
    "listener now prefers a dual-stack [::] bind (socket2, IPV6_V6ONLY off) with the old "
    "0.0.0.0 bind as fallback, so a peer connecting over IPv6 can be served. On this Mac the "
    "listener binds [::] and still serves 127.0.0.1. ERR-007 records the observed os error "
    "65. 11 new tests (network 68 -> 79), full suite 405 green, build warning-free. Docs "
    "updated: DEVELOPER_GUIDE troubleshooting gains the fe80:: entry, the transfer flow "
    "documents candidate selection and the dual-stack listener, and the counts are "
    "405/99. Pending: rebuild + reinstall the .exe on the Windows laptop and retest both send "
    "directions on hardware.",
    "Assistant",
    "bug,network,ipv6,link-local,connect-fix",
]
logs.append(log_entry)

# ---------------------------------------------------------------------------
# Overview: test coverage + its timestamp
# ---------------------------------------------------------------------------
overview = wb["Overview"]
for row in range(2, overview.max_row + 1):
    field = overview.cell(row=row, column=1).value
    if field == "Test Coverage":
        overview.cell(row=row, column=2).value = (
            "405 Rust tests (12 of them two-device end-to-end across a real socket; 13 in the "
            "new files crate; 11 connect-candidate and dual-stack listener tests in the "
            "network crate), 99 frontend tests, 0 build warnings, command-surface parity "
            "clean at 44/44"
        )
        overview.cell(row=row, column=4).value = f"{TODAY} 23:10"

wb.save("WorkspaceClone_Tracking.xlsx")
print("saved:")
print("  bugs rows    :", bugs.max_row - 1)
print("  errors rows  :", errors.max_row - 1)
print("  decisions    :", decisions.max_row - 1)
print("  runs rows    :", runs.max_row - 1)
print("  logs rows    :", logs.max_row - 1)