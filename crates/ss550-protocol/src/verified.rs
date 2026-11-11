//! The verified command registry — the safe-mode list the driver enforces.
//!
//! `AGENTS.md` §4 says, verbatim:
//!
//! > **NEVER send untested raw commands to the device.** All commands must
//! > be documented, unit tested with golden vectors and integration tested on
//! > a dedicated test device.
//!
//! This module is the machine-readable half of that rule: for every command
//! the protocol crate can encode, it says whether a **test deck** has proven
//! it, and where the evidence lives.
//!
//! The human-readable half is `docs/protocol/verified.md`. The two must
//! agree — same names, same status, same evidence text — and a test fails
//! when they disagree, so the document cannot drift away from the code.
//!
//! This crate stays pure (no I/O, no clock, no USB), so it owns the
//! *registry*; the driver owns the *enforcement*.

/// A command the host can send to the deck.
///
/// Every command the protocol crate can encode is listed here, whether or not
/// it has been proven on hardware. The driver refuses to send the ones that
/// have not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Command {
    /// `CRT|DIS` — wakes the display.
    DisplayOn,
    /// `CRT|LIG` — sets the backlight, 0 to 100.
    Brightness,
    /// `CRT|BAT` + JPEG chunks + `CRT|STP` — paints the keys.
    ImageTransfer,
    /// `CRT|CLE 0xFF` + `CRT|STP` — blanks every key.
    ClearAll,
    /// `CRT|CLE <key>` + `CRT|STP` — blanks one key.
    ClearKey,
    /// `CRT|CONNECT` — the reference implementation's keep-alive.
    KeepAlive,
    /// `CRT|HAN` — puts the display to sleep.
    Sleep,
    /// bare `HAN` — the connect-time greeting.
    Handshake,
    /// `CRT|VER` — asks the firmware for its version.
    FirmwareVersion,
    /// `CRT|MOD` — switches the device mode.
    Mode,
}

impl Command {
    /// The snake_case name used in `docs/protocol/verified.md`.
    ///
    /// This is the identity of the command everywhere outside the wire: the
    /// registry, the document and the driver's refusal message all use it.
    ///
    /// ```
    /// use ss550_protocol::Command;
    ///
    /// assert_eq!(Command::ImageTransfer.name(), "image_transfer");
    /// ```
    pub const fn name(self) -> &'static str {
        match self {
            Self::DisplayOn => "display_on",
            Self::Brightness => "brightness",
            Self::ImageTransfer => "image_transfer",
            Self::ClearAll => "clear_all",
            Self::ClearKey => "clear_key",
            Self::KeepAlive => "keep_alive",
            Self::Sleep => "sleep",
            Self::Handshake => "handshake",
            Self::FirmwareVersion => "firmware_version",
            Self::Mode => "mode",
        }
    }

    /// The tag on the wire.
    ///
    /// Two commands can share a tag — `clear_all` and `clear_key` are both
    /// `CLE`, `sleep` and `handshake` are both `HAN` — so the tag identifies
    /// the *bytes*, not the command. Use [`Command::name`] to identify the
    /// command.
    ///
    /// ```
    /// use ss550_protocol::Command;
    ///
    /// assert_eq!(Command::ClearAll.tag(), "CLE");
    /// assert_eq!(Command::ClearKey.tag(), "CLE");
    /// ```
    pub const fn tag(self) -> &'static str {
        match self {
            Self::DisplayOn => "DIS",
            Self::Brightness => "LIG",
            Self::ImageTransfer => "BAT",
            Self::ClearAll => "CLE",
            Self::ClearKey => "CLE",
            Self::KeepAlive => "CONNECT",
            Self::Sleep => "HAN",
            Self::Handshake => "HAN",
            Self::FirmwareVersion => "VER",
            Self::Mode => "MOD",
        }
    }

    /// Whether the driver may send this command.
    ///
    /// ```
    /// use ss550_protocol::Command;
    ///
    /// assert!(Command::DisplayOn.is_verified());
    /// assert!(!Command::FirmwareVersion.is_verified());
    /// ```
    pub const fn is_verified(self) -> bool {
        matches!(
            self.info().verification,
            Verification::Verified { evidence: _ }
        )
    }

    /// Everything the registry knows about this command.
    ///
    /// ```
    /// use ss550_protocol::{Command, Verification};
    ///
    /// let info = Command::ClearKey.info();
    /// assert_eq!(info.tag, "CLE");
    /// assert!(matches!(
    ///     info.verification,
    ///     Verification::Unverified { .. }
    /// ));
    /// ```
    pub const fn info(self) -> CommandInfo {
        let (summary, verification) = match self {
            Self::DisplayOn => (
                "Wakes the display. No persistent side effect, so it doubles as the heartbeat.",
                Verification::Verified {
                    evidence: "docs/protocol/image-upload.md — the single-image path, step 1, verified live",
                },
            ),
            Self::Brightness => (
                "Sets the backlight, 0 to 100.",
                Verification::Verified {
                    evidence: "docs/protocol/image-upload.md — --bright 30 visibly dims the deck",
                },
            ),
            Self::ImageTransfer => (
                "Paints the keys: BAT header, the JPEG in chunks, then exactly one STP.",
                Verification::Verified {
                    evidence: "docs/protocol/image-upload.md — the single-image path renders all 15 keys",
                },
            ),
            Self::ClearAll => (
                "Blanks every key: CLE with the 0xFF selector, then one STP.",
                Verification::Verified {
                    evidence: "docs/protocol/CHANGELOG.md — clearing every key was exercised on the deck",
                },
            ),
            Self::ClearKey => (
                "Blanks one key: CLE with the firmware key, then one STP.",
                Verification::Unverified {
                    reason: "never sent to a deck: live clear-per-key is still pending (docs/protocol/CHANGELOG.md)",
                },
            ),
            Self::Sleep => (
                "Puts the display to sleep: CRT|HAN.",
                Verification::Unverified {
                    reason: "encoded from the reference implementation, never exercised on a deck (docs/protocol/image-upload.md)",
                },
            ),
            Self::KeepAlive => (
                "The reference implementation's CONNECT keep-alive. This driver does not use it.",
                Verification::Unverified {
                    reason: "the driver heartbeats with DIS instead, so this one was never needed (docs/protocol/CHANGELOG.md)",
                },
            ),
            Self::Handshake => (
                "Connect-time greeting: a bare HAN, with no CRT prefix.",
                Verification::Unverified {
                    reason: "the driver never handshakes: the deck is waited for, not greeted (docs/protocol/image-upload.md)",
                },
            ),
            Self::FirmwareVersion => (
                "Asks the firmware for its version: VER. Never send it.",
                Verification::Unverified {
                    reason: "writing it halts the USB endpoint, so it must never be sent (docs/troubleshooting/deck-stuck.md)",
                },
            ),
            Self::Mode => (
                "Switches the device mode: MOD.",
                Verification::Unverified {
                    reason: "static-only, part of the unverified profile path (docs/protocol/CHANGELOG.md)",
                },
            ),
        };

        CommandInfo {
            command: self,
            tag: self.tag(),
            summary,
            verification,
        }
    }
}

/// Why a command may or may not be sent.
///
/// The distinction is **hardware** evidence, not static analysis: a recovered
/// disassembly or a reference implementation is a hypothesis, not a proof.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verification {
    /// Exercised on a real deck. `evidence` points at the document that proves it.
    Verified {
        /// The evidence, character for character as `docs/protocol/verified.md`
        /// records it.
        evidence: &'static str,
    },
    /// Never exercised. `reason` says why and points at the document that records the gap.
    Unverified {
        /// Why the command is still unproven, character for character as
        /// `docs/protocol/verified.md` records it.
        reason: &'static str,
    },
}

/// Everything the registry knows about one command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandInfo {
    /// The command this describes.
    pub command: Command,
    /// The tag on the wire.
    pub tag: &'static str,
    /// One line: what the command does to the deck.
    pub summary: &'static str,
    /// Whether a test deck has proven it, and where the evidence lives.
    pub verification: Verification,
}

/// Every command, exactly once each, in the order the document lists them.
///
/// The order matches `docs/protocol/verified.md`: the four commands a test
/// deck has proven first, then the ones that are still pending.
pub const COMMANDS: &[Command] = &[
    Command::DisplayOn,
    Command::Brightness,
    Command::ImageTransfer,
    Command::ClearAll,
    Command::ClearKey,
    Command::Sleep,
    Command::KeepAlive,
    Command::Handshake,
    Command::FirmwareVersion,
    Command::Mode,
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Every command, listed by hand: a new variant that nobody adds to
    /// [`COMMANDS`] must fail this test instead of sailing through.
    const ALL: [Command; 10] = [
        Command::DisplayOn,
        Command::Brightness,
        Command::ImageTransfer,
        Command::ClearAll,
        Command::ClearKey,
        Command::KeepAlive,
        Command::Sleep,
        Command::Handshake,
        Command::FirmwareVersion,
        Command::Mode,
    ];

    #[test]
    fn the_registry_holds_every_command_exactly_once() {
        assert_eq!(COMMANDS.len(), ALL.len());
        for command in ALL {
            let count = COMMANDS.iter().filter(|c| **c == command).count();
            assert_eq!(count, 1, "{} must appear exactly once", command.name());
        }
    }

    #[test]
    fn the_names_are_unique() {
        let mut names: HashSet<&str> = HashSet::new();
        for command in COMMANDS {
            assert!(
                names.insert(command.name()),
                "duplicate name: {}",
                command.name()
            );
        }
    }

    #[test]
    fn the_names_match_the_document() {
        let expected = [
            (Command::DisplayOn, "display_on"),
            (Command::Brightness, "brightness"),
            (Command::ImageTransfer, "image_transfer"),
            (Command::ClearAll, "clear_all"),
            (Command::ClearKey, "clear_key"),
            (Command::KeepAlive, "keep_alive"),
            (Command::Sleep, "sleep"),
            (Command::Handshake, "handshake"),
            (Command::FirmwareVersion, "firmware_version"),
            (Command::Mode, "mode"),
        ];
        for (command, name) in expected {
            assert_eq!(command.name(), name);
        }
    }

    #[test]
    fn the_tags_match_the_document() {
        let expected = [
            (Command::DisplayOn, "DIS"),
            (Command::Brightness, "LIG"),
            (Command::ImageTransfer, "BAT"),
            (Command::ClearAll, "CLE"),
            (Command::ClearKey, "CLE"),
            (Command::KeepAlive, "CONNECT"),
            (Command::Sleep, "HAN"),
            (Command::Handshake, "HAN"),
            (Command::FirmwareVersion, "VER"),
            (Command::Mode, "MOD"),
        ];
        for (command, tag) in expected {
            assert_eq!(command.tag(), tag);
        }
    }

    #[test]
    fn every_verified_command_carries_evidence_and_every_other_one_a_reason() {
        for command in COMMANDS {
            match command.info().verification {
                Verification::Verified { evidence } => assert!(
                    !evidence.trim().is_empty(),
                    "{} is verified but carries no evidence",
                    command.name()
                ),
                Verification::Unverified { reason } => assert!(
                    !reason.trim().is_empty(),
                    "{} is unverified but carries no reason",
                    command.name()
                ),
            }
        }
    }

    #[test]
    fn every_command_carries_a_summary_and_its_own_identity() {
        for command in COMMANDS {
            let info = command.info();
            assert_eq!(info.command, *command);
            assert_eq!(info.tag, command.tag());
            assert!(
                !info.summary.trim().is_empty(),
                "{} has no summary",
                command.name()
            );
        }
    }

    #[test]
    fn is_verified_agrees_with_the_registry() {
        for command in COMMANDS {
            let expected = matches!(command.info().verification, Verification::Verified { .. });
            assert_eq!(
                command.is_verified(),
                expected,
                "{} disagrees with its own entry",
                command.name()
            );
        }
    }

    #[test]
    fn only_the_four_live_commands_are_verified() {
        let verified: Vec<&str> = COMMANDS
            .iter()
            .filter(|command| command.is_verified())
            .map(|command| command.name())
            .collect();
        assert_eq!(
            verified,
            ["display_on", "brightness", "image_transfer", "clear_all"]
        );
    }
}
