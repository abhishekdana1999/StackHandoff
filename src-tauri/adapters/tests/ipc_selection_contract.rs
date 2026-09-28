//! The contract between the TypeScript that builds a capture selection and the
//! Rust that deserializes it.
//!
//! ## Why this file exists
//!
//! Sending a workspace from one machine to another failed with
//!
//! ```text
//! invalid args `selection` for command `capture_workspace`:
//! missing field `file_transfer`
//! ```
//!
//! The cause was not a missing field. Both types were complete and agreed on
//! every *name*; they disagreed on the *convention*. Rust read the selection
//! as snake_case (`include_applications`, `browser_urls`, `source_path`) and the
//! UI wrote it as camelCase (`includeApplications`, `browserUrls`,
//! `sourcePath`). Serde ignores unrecognised keys, so every mismatched field
//! arrived as an empty list rather than an error.
//!
//! Two things made this expensive to diagnose:
//!
//! 1. `file_transfer` is the *first* missing field in `Policy`'s declaration
//!    order, and `Policy` is nested. Its failure short-circuited the outer
//!    deserialization, so the error named a field the caller was in fact
//!    sending, under a name it was also sending. Fixing only that field would
//!    have produced a second error immediately after.
//! 2. Nothing in the build could see it. `tsc` checks the TypeScript against
//!    its own types, `cargo test` builds `CaptureSelection` in Rust where
//!    naming is irrelevant, and the command-parity gate compares command
//!    *names*. Three green gates, and the bug lived in the space between them.
//!
//! So the gate is placed in that space. The tests below read the real
//! `src/types/index.ts` and compare its field names against the names serde
//! will actually accept. Renaming a field in either language now fails here
//! rather than at a user's capture.

use workspace_clone_adapters::traits::{ApprovedCommand, CaptureSelection, SelectedProject};
use workspace_clone_core::manifest::Policy;

/// The payload `CaptureScreen.handleCapture` builds, spelled exactly as
/// `emptySelection()` in `src/store/useAppStore.ts` writes it.
///
/// Written out by hand on purpose, and cross-checked against the TypeScript
/// source by `the_typescript_and_the_rust_types_still_agree` below. This test
/// is what proves the values *arrive*; that one is what proves the names have
/// not drifted apart since.
const THE_UI_ACTUALLY_SENDS: &str = r#"{
  "projects": [
    {
      "id": "project-1",
      "name": "openshorts",
      "sourcePath": "/Users/someone/code/openshorts",
      "destinationLocationId": "code"
    }
  ],
  "includeApplications": ["browser", "vscode", "terminal"],
  "browserUrls": ["https://example.com/one", "https://example.com/two"],
  "terminalDirs": ["/Users/someone/code/openshorts"],
  "terminalCommands": [
    {
      "label": "Run the tests",
      "command": "npm test",
      "workingDirectory": "/Users/someone/code/openshorts"
    }
  ],
  "envVarNames": ["PATH", "NODE_ENV"],
  "policy": {
    "fileTransfer": "none",
    "clipboard": "excluded",
    "automaticCommandExecution": false,
    "secretValuesIncluded": false
  }
}"#;

#[test]
fn the_selection_the_ui_sends_deserializes_completely() {
    let selection: CaptureSelection =
        serde_json::from_str(THE_UI_ACTUALLY_SENDS).expect("the UI's payload must deserialize");

    assert_eq!(selection.include_applications, ["browser", "vscode", "terminal"]);
    assert_eq!(
        selection.browser_urls,
        ["https://example.com/one", "https://example.com/two"],
        "URLs the user typed must survive the trip; an empty list here is the \
         silent data loss that made this bug look like a browser-adapter fault"
    );
    assert_eq!(selection.terminal_dirs, ["/Users/someone/code/openshorts"]);
    assert_eq!(selection.env_var_names, ["PATH", "NODE_ENV"]);

    let project = selection
        .project_at("project-1")
        .expect("the selected project must be found");
    assert_eq!(project.source_path, "/Users/someone/code/openshorts");
    assert_eq!(project.destination_location_id, "code");

    let command = selection
        .terminal_commands
        .first()
        .expect("the approved command must be present");
    assert_eq!(command.working_directory.as_deref(), Some("/Users/someone/code/openshorts"));

    assert!(selection.includes("browser"), "include_applications must be readable");
    assert!(!selection.includes("nonexistent"));
}

#[test]
fn a_misspelled_key_is_refused_instead_of_quietly_captured_nothing() {
    // The whole reason this is a bug worth a test rather than a rename. Serde's
    // default is to ignore a key it does not recognise, which turns any typo in
    // either language into a successful capture that is missing what the user
    // selected. `deny_unknown_fields` makes the typo loud.
    let typo = r#"{
      "projects": [],
      "includeApplication": ["browser"],
      "browserUrls": [],
      "terminalDirs": [],
      "terminalCommands": [],
      "envVarNames": [],
      "policy": {
        "fileTransfer": "none",
        "clipboard": "excluded",
        "automaticCommandExecution": false,
        "secretValuesIncluded": false
      }
    }"#;

    let err = serde_json::from_str::<CaptureSelection>(typo)
        .expect_err("a misspelled key must not deserialize");
    let message = err.to_string();
    assert!(
        message.contains("includeApplication"),
        "the error must name the key that was wrong, so the fix is obvious: {message}"
    );
}

#[test]
fn the_policy_reads_camel_case_but_still_writes_snake_case() {
    // `Policy` is the one type that crosses two boundaries with two
    // conventions: camelCase inside a `CaptureSelection` arriving over IPC, and
    // snake_case inside the manifest, which is the on-the-wire format by
    // decision. A blanket `rename_all` would have fixed the input and quietly
    // rewritten the manifest format, including for manifests already stored.
    let from_camel: Policy = serde_json::from_str(
        r#"{
          "fileTransfer": "explicit",
          "clipboard": "excluded",
          "automaticCommandExecution": false,
          "secretValuesIncluded": false
        }"#,
    )
    .expect("the UI's camelCase policy must deserialize");

    let written = serde_json::to_value(&from_camel).expect("serialize");
    assert_eq!(
        written.get("file_transfer").and_then(|v| v.as_str()),
        Some("explicit"),
        "the manifest must keep its snake_case wire format: {written}"
    );
    assert!(
        written.get("fileTransfer").is_none(),
        "a camelCase key in the manifest would be a wire format change: {written}"
    );
}

#[test]
fn a_manifest_round_trip_still_reads() {
    // Guards the aliases against breaking the older spelling. Anything that
    // wrote or read a manifest directly, including the receiver's validation
    // path, uses snake_case and must keep working.
    let policy = Policy::default();
    let json = serde_json::to_string(&policy).expect("serialize");
    let reread: Policy = serde_json::from_str(&json).expect("the manifest must still deserialize");
    assert_eq!(
        serde_json::to_string(&reread).unwrap(),
        json,
        "a manifest policy must survive a serialize/deserialize round trip unchanged"
    );
}

// ---------------------------------------------------------------------------
// Cross-language field-name check
// ---------------------------------------------------------------------------

/// The repository root, so this test reads the same TypeScript the app builds.
fn repo_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the repository root must exist")
}

fn read_typescript() -> String {
    let path = repo_root().join("src/types/index.ts");
    std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "could not read {}: {e}\n\
             This test exists to compare the Rust types against the TypeScript \
             they talk to. If the file moved, point this test at its new home \
             rather than deleting it -- it is the only check that the two \
             languages agree on field names.",
            path.display()
        )
    })
}

/// The field names declared by a TypeScript `interface`.
///
/// Comments are stripped first so a `}` inside a doc comment cannot end the
/// search early, and only `name:` / `name?:` at the start of a line counts, so
/// a type annotation containing a colon is not mistaken for a field.
fn ts_interface_fields(source: &str, interface: &str) -> Vec<String> {
    let start = source
        .lines()
        .position(|l| l.contains(&format!("interface {interface} {{")))
        .unwrap_or_else(|| {
            panic!("no `interface {interface} {{` in src/types/index.ts; this test needs updating")
        });

    let body: String = source
        .lines()
        .skip(start + 1)
        .take_while(|l| !l.trim_start().starts_with('}'))
        .collect::<Vec<_>>()
        .join("\n");

    let mut in_block_comment = false;
    body.lines()
        .filter_map(|line| {
            let line = strip_comments(line, &mut in_block_comment)?;
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            let name = line.split(':').next()?.trim().trim_end_matches('?').trim();
            if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                None
            } else {
                Some(name.to_string())
            }
        })
        .collect()
}

/// Remove a line's comments, tracking multi-line `/* */` state across lines.
fn strip_comments(line: &str, in_block: &mut bool) -> Option<String> {
    let mut out = String::new();
    let mut chars = line.chars().peekable();

    while let Some(c) = chars.next() {
        if *in_block {
            if c == '*' && chars.peek() == Some(&'/') {
                chars.next();
                *in_block = false;
            }
            continue;
        }
        if c == '/' && chars.peek() == Some(&'*') {
            chars.next();
            *in_block = true;
            continue;
        }
        if c == '/' && chars.peek() == Some(&'/') {
            break;
        }
        out.push(c);
    }

    if out.trim().is_empty() {
        None
    } else {
        Some(out)
    }
}

/// `automatic_command_execution` -> `automaticCommandExecution`, matching serde's
/// `rename_all = "camelCase"`: every segment after the first is capitalised.
///
/// Capitalising the later segments is the part that matters. Simply lowercasing
/// the first character and deleting the underscores turns
/// `automatic_command_execution` into `automaticcommandexecution`, which would
/// have quietly agreed with nothing and reported a mismatch on every multi-word
/// field in the struct.
fn to_camel(snake: &str) -> String {
    let mut out = String::new();
    for (index, segment) in snake.split('_').enumerate() {
        let mut chars = segment.chars();
        match chars.next() {
            None => continue,
            Some(first) => {
                if index == 0 {
                    out.extend(first.to_lowercase());
                } else {
                    out.extend(first.to_uppercase());
                }
                out.push_str(chars.as_str());
            }
        }
    }
    out
}

fn assert_fields_agree(rust_keys: Vec<String>, ts_interface: &str, source: &str, context: &str) {
    let ts_fields = ts_interface_fields(source, ts_interface);
    let expected: Vec<String> = rust_keys.iter().map(|k| to_camel(k)).collect();

    // Compared as sets, not sequences. `serde_json::Value` is a `BTreeMap`
    // unless the `preserve_order` feature is enabled, so these come back
    // alphabetically while TypeScript lists them in declaration order. Field
    // order carries no meaning across a JSON boundary; only the names do.
    let mut expected_sorted = expected.clone();
    let mut ts_sorted = ts_fields.clone();
    expected_sorted.sort();
    ts_sorted.sort();

    assert_eq!(
        expected_sorted, ts_sorted,
        "\n{context}\n\
         Rust accepts {expected_sorted:?}\n\
         TypeScript declares {ts_sorted:?}\n\
         A field added or renamed on one side only means the UI sends something \
         the receiver silently drops, so the capture comes out incomplete with \
         no error. Update both together."
    );
}

#[test]
fn the_selection_serializes_in_camel_case() {
    // A separate assertion because the field-name comparison below cannot catch
    // this on its own: it converts snake_case to camelCase before comparing, so
    // a struct that had *lost* its `rename_all` would still agree with the
    // TypeScript and the check would pass. That was verified by reverting the
    // fix and re-running -- this test failed, the comparison did not. The
    // attribute is the whole cause of the original bug, so it gets pinned
    // directly rather than inferred.
    let keys: Vec<String> = serde_json::to_value(CaptureSelection::default())
        .expect("serialize")
        .as_object()
        .expect("an object")
        .keys()
        .cloned()
        .collect();

    for expected in [
        "includeApplications",
        "browserUrls",
        "terminalDirs",
        "terminalCommands",
        "envVarNames",
    ] {
        assert!(
            keys.contains(&expected.to_string()),
            "`{expected}` must be the key serde uses; got {keys:?}"
        );
    }
    for wrong in [
        "include_applications",
        "browser_urls",
        "terminal_dirs",
        "terminal_commands",
        "env_var_names",
    ] {
        assert!(
            !keys.contains(&wrong.to_string()),
            "`{wrong}` means `rename_all = \"camelCase\"` was dropped from \
             CaptureSelection, which is the exact cause of the send failure"
        );
    }
}

#[test]
fn the_typescript_and_the_rust_types_still_agree() {
    let ts = read_typescript();

    // Derived from serde rather than hardcoded, so this tracks whatever
    // `rename_all` actually does instead of restating the intent.
    let selection_keys: Vec<String> = serde_json::to_value(CaptureSelection::default())
        .expect("a default selection must serialize")
        .as_object()
        .expect("an object")
        .keys()
        .cloned()
        .collect();
    assert_fields_agree(
        selection_keys,
        "CaptureSelection",
        &ts,
        "CaptureSelection",
    );

    let project = SelectedProject {
        id: "p".into(),
        name: "p".into(),
        source_path: "/p".into(),
        destination_location_id: "code".into(),
    };
    let project_keys: Vec<String> = serde_json::to_value(project)
        .expect("serialize")
        .as_object()
        .expect("an object")
        .keys()
        .cloned()
        .collect();
    assert_fields_agree(project_keys, "SelectedProject", &ts, "SelectedProject");

    let command = ApprovedCommand {
        label: "l".into(),
        command: "c".into(),
        working_directory: None,
    };
    let command_keys: Vec<String> = serde_json::to_value(command)
        .expect("serialize")
        .as_object()
        .expect("an object")
        .keys()
        .cloned()
        .collect();
    assert_fields_agree(command_keys, "ApprovedCommand", &ts, "ApprovedCommand");

    // `Policy` is compared through its snake_case form, because that is what
    // serde writes; the camelCase aliases are covered by
    // `the_policy_reads_camel_case_but_still_writes_snake_case` above.
    let policy_keys: Vec<String> = serde_json::to_value(Policy::default())
        .expect("serialize")
        .as_object()
        .expect("an object")
        .keys()
        .cloned()
        .collect();
    assert_fields_agree(policy_keys, "CapturePolicy", &ts, "Policy / CapturePolicy");
}
