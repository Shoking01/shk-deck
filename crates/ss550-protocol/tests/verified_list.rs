//! The verified list and the registry must not drift apart.
//!
//! `docs/protocol/verified.md` is the file a person reads before touching the
//! deck; `ss550_protocol::COMMANDS` is what the driver enforces. If they
//! disagree, one of them is lying — and the lying one is the one that decides
//! whether a command may reach a device that a bad packet can brick.
//!
//! So this test compares them: same commands, same tags, same status, and the
//! evidence text the registry carries must be the text the document prints.

use ss550_protocol::{Command, Verification, COMMANDS};

/// The verified list, read at compile time from the repository.
const DOCUMENT: &str = include_str!("../../../docs/protocol/verified.md");

/// One row of the table in `docs/protocol/verified.md`.
struct Row {
    name: String,
    tag: String,
    status: String,
    note: String,
}

/// Strips the markdown backticks a cell may carry.
fn plain(cell: &str) -> String {
    cell.trim().trim_matches('`').trim().to_string()
}

/// The rows of the table under `## The list`.
///
/// The section is scoped so a second table elsewhere in the document cannot
/// be mistaken for a command row.
fn rows() -> Vec<Row> {
    let mut inside = false;
    let mut rows = Vec::new();

    for line in DOCUMENT.lines() {
        let line = line.trim();

        if line.starts_with("## ") {
            inside = line == "## The list";
            continue;
        }
        if !inside || !line.starts_with('|') {
            continue;
        }

        let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
        if cells.len() != 4 {
            continue;
        }
        // The header row and the separator row are not commands.
        if cells[0] == "Command" || cells[0].starts_with('-') {
            continue;
        }

        rows.push(Row {
            name: plain(cells[0]),
            tag: plain(cells[1]),
            status: plain(cells[2]),
            note: plain(cells[3]),
        });
    }

    rows
}

/// The text the registry carries for a command: its evidence, or its reason.
fn registry_text(command: Command) -> &'static str {
    match command.info().verification {
        Verification::Verified { evidence } => evidence,
        Verification::Unverified { reason } => reason,
    }
}

#[test]
fn the_document_lists_every_command_the_registry_knows() {
    let rows = rows();
    assert_eq!(
        rows.len(),
        COMMANDS.len(),
        "the document has {} rows and the registry has {} commands",
        rows.len(),
        COMMANDS.len()
    );

    for command in COMMANDS {
        let row = rows
            .iter()
            .find(|row| row.name == command.name())
            .unwrap_or_else(|| {
                panic!(
                    "the document does not list {} — the registry and the \
                     verified list must name the same commands",
                    command.name()
                )
            });

        assert_eq!(
            row.tag,
            command.tag(),
            "the document says {} is {} and the registry says {}",
            command.name(),
            row.tag,
            command.tag()
        );

        let expected_status = match command.info().verification {
            Verification::Verified { .. } => "verified",
            Verification::Unverified { .. } => "unverified",
        };
        assert_eq!(
            row.status,
            expected_status,
            "the document calls {} {} and the registry calls it {}",
            command.name(),
            row.status,
            expected_status
        );

        assert!(
            row.note.contains(registry_text(*command)),
            "the document's note for {} does not carry the registry's text: \
             the document says {:?} and the registry says {:?}",
            command.name(),
            row.note,
            registry_text(*command)
        );
    }
}

#[test]
fn the_document_lists_no_command_the_registry_does_not_know() {
    for row in rows() {
        assert!(
            COMMANDS.iter().any(|command| command.name() == row.name),
            "the document lists {}, which is not in the registry",
            row.name
        );
    }
}

#[test]
fn the_document_and_the_registry_agree_on_what_is_proven() {
    let rows = rows();
    for command in COMMANDS {
        let row = rows
            .iter()
            .find(|row| row.name == command.name())
            .unwrap_or_else(|| panic!("the document does not list {}", command.name()));
        assert_eq!(
            row.status == "verified",
            command.is_verified(),
            "{} is {} in the document and verified={} in the registry",
            command.name(),
            row.status,
            command.is_verified()
        );
    }
}
