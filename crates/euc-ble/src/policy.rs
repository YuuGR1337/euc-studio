//! Whether this process may write to a real wheel at all.

use serde::Serialize;

/// The environment variable that arms real writes for one wheel.
pub const OVERRIDE_ENV: &str = "EUC_BLE_REAL_WRITES";
/// What its value starts with; the wheel's BLE identifier follows.
pub const OVERRIDE_PREFIX: &str = "swd-backup-done:";

/// The override value that arms real writes to the wheel with this BLE identifier.
///
/// Setting it states that a full SWD backup of that wheel's flash (bootloader included) exists and
/// that its owner approved this run (SAFETY.md). It names one wheel, so it cannot arm another.
pub fn override_value(peer: &str) -> String {
    format!("{OVERRIDE_PREFIX}{peer}")
}

/// Real-write permission for one connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum WritePolicy {
    /// The default: notifications are read, every write is refused.
    ListenOnly,
    /// The override names this wheel: writes go through the guard.
    Armed {
        /// The wheel's BLE identifier.
        peer: String,
    },
}

impl WritePolicy {
    /// The policy for wheel `peer` from [`OVERRIDE_ENV`].
    pub fn from_env(peer: &str) -> Self {
        Self::from_value(std::env::var(OVERRIDE_ENV).ok().as_deref(), peer)
    }

    /// The policy for wheel `peer` from an override value (exact match only).
    pub fn from_value(value: Option<&str>, peer: &str) -> Self {
        match value {
            Some(value) if !peer.trim().is_empty() && value == override_value(peer) => {
                Self::Armed { peer: peer.to_owned() }
            }
            _ => Self::ListenOnly,
        }
    }

    /// True when writes may reach the wheel (still subject to the guard).
    pub const fn allows_writes(&self) -> bool {
        matches!(self, Self::Armed { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PEER: &str = "8F0E3C4A-1B2C-4D5E-9F60-718293A4B5C6";

    #[test]
    fn listen_only_unless_the_value_names_this_wheel() {
        assert_eq!(WritePolicy::from_value(None, PEER), WritePolicy::ListenOnly);
        assert_eq!(WritePolicy::from_value(Some("1"), PEER), WritePolicy::ListenOnly);
        assert_eq!(WritePolicy::from_value(Some("swd-backup-done:"), PEER), WritePolicy::ListenOnly);
        assert_eq!(WritePolicy::from_value(Some("swd-backup-done:OTHER"), PEER), WritePolicy::ListenOnly);
        assert_eq!(WritePolicy::from_value(Some(&override_value(PEER).to_lowercase()), PEER), WritePolicy::ListenOnly);
        assert_eq!(WritePolicy::from_value(Some("swd-backup-done:"), ""), WritePolicy::ListenOnly);
        let armed = WritePolicy::from_value(Some(&override_value(PEER)), PEER);
        assert_eq!(armed, WritePolicy::Armed { peer: PEER.into() });
        assert!(armed.allows_writes() && !WritePolicy::ListenOnly.allows_writes());
    }

    #[test]
    fn the_test_environment_is_not_armed() {
        // CI and developer shells must never carry the override.
        assert!(!WritePolicy::from_env(PEER).allows_writes());
    }
}
