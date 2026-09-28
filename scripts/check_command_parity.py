#!/usr/bin/env python3
"""
Check that the Rust command surface and the TypeScript wrapper surface agree.

Tauri registers a command under the *function's* name. The module in
`workspace_clone_commands::transfer::send_workspace` is a Rust path, not part of
the name the frontend has to use, so the comparison has to drop it.

Three ways this can be wrong, and what each one means:

* A command registered in Rust that no wrapper calls -- dead code on the Rust
  side. It compiles, it is covered by tests, and no user can reach it.
* A wrapper naming a command Rust does not register -- the call fails at
  runtime with "command not found". TypeScript cannot catch this: `invoke` takes
  a string, and a string is always well typed.
* A duplicated name across two modules, where the second registration shadows
  the first. Not a parity problem, but the one thing a name-set comparison
  cannot see, so it is reported.

Exits non-zero on any finding, so it can be run in CI.
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
APP = ROOT / "src-tauri" / "app" / "src" / "lib.rs"
IPC = ROOT / "src" / "lib" / "ipc.ts"

# `module::command` inside the invoke_handler macro.
HANDLER = re.compile(r"workspace_clone_commands::(\w+)::(\w+)")
# `invoke('command_name'`. The name is what the frontend must match.
INVOKE = re.compile(r"invoke\(\s*'([a-z0-9_]+)'")

# `generate_handler![...]` spans the registrations; anything matching
# `workspace_clone_commands::` outside it would be a plain function call and
# would wrongly count as a registration.
def handler_body(source: str) -> str:
    start = source.find("generate_handler!")
    if start == -1:
        sys.exit("could not find generate_handler! in app/src/lib.rs")
    depth, i = 0, start
    while i < len(source):
        if source[i] == "[":
            depth += 1
        elif source[i] == "]":
            depth -= 1
            if depth == 0:
                return source[start:i]
        i += 1
    sys.exit("generate_handler! is not closed in app/src/lib.rs")


def main() -> int:
    pairs = HANDLER.findall(handler_body(APP.read_text()))
    registered = [fn for _mod, fn in pairs]
    called = sorted(set(INVOKE.findall(IPC.read_text())))

    reg_set, dupes = set(), set()
    for name in registered:
        if name in reg_set:
            dupes.add(name)
        reg_set.add(name)

    never_called = sorted(reg_set - set(called))
    not_registered = sorted(set(called) - reg_set)

    print(f"registered in Rust : {len(registered)} entries, {len(reg_set)} distinct names")
    print(f"called from TS     : {len(called)} distinct names")

    if dupes:
        print(f"\nDUPLICATE REGISTRATION ({len(dupes)}) -- the second shadows the first:")
        for name in sorted(dupes):
            modules = sorted(m for m, fn in pairs if fn == name)
            print(f"    {name}  <- {', '.join(modules)}")
        print("    the frontend cannot tell which one it is calling.")

    if never_called:
        print(f"\nREGISTERED BUT NEVER CALLED FROM TS ({len(never_called)}):")
        for name in never_called:
            print(f"    {name}")
        print("    reachable by no user. Still compiled, still tested, still dead.")

    if not_registered:
        print(f"\nCALLED FROM TS BUT NOT REGISTERED ({len(not_registered)}):")
        for name in not_registered:
            print(f"    {name}")
        print("    each of these fails at runtime with 'command not found'.")

    if dupes or never_called or not_registered:
        print("\nPARITY BROKEN")
        return 1

    print("\nPARITY OK: every command Rust registers is called, and every call resolves.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
