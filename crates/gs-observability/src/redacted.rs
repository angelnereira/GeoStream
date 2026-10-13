//! Secret newtypes with redacted `Debug`/`Display` and `Zeroize` on drop (docs/10 §8.2).
//!
//! Nothing of these types may reach a log, a metric label, a trace attribute or an error
//! message. They exist so that the compiler forbids it: the inner value cannot be obtained
//! without calling `expose`, which the codebase must never do while logging.
#![forbid(unsafe_code)]

use std::fmt;
use zeroize::{Zeroize, Zeroizing};

/// Macro that defines a secret newtype: redacted `Debug`/`Display`, zeroized on drop.
macro_rules! define_secret {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, PartialEq, Eq)]
        pub struct $name(Zeroizing<String>);

        impl $name {
            /// Wraps a secret value. The value is never rendered by `Debug` or `Display`.
            pub fn new(value: impl Into<String>) -> Self {
                Self(Zeroizing::new(value.into()))
            }

            /// Exposes the secret value for the single operation that needs it (hashing,
            /// signing, comparison). Never pass the result to a log or a metric.
            pub fn expose(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(concat!(stringify!($name), "(<redacted>)"))
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("<redacted>")
            }
        }
    };
}

define_secret!(
    ApiKeySecret,
    "Secret of an application API key (`gsk_…`, docs/06 §3.1)."
);
define_secret!(
    AccessToken,
    "Device JWT (`eyJ…`, docs/06 §4). Only its claims are ever inspected."
);
define_secret!(Ticket, "Single-use stream ticket (docs/06 §5).");
define_secret!(
    InvitationCode,
    "Invitation code in clear text (docs/05 §6)."
);
define_secret!(
    WebhookSecret,
    "Webhook signing secret (`whsec_…`, ADR-0015)."
);

/// Explicitly forgets a secret value in place.
pub fn forget<T: Zeroize>(value: &mut T) {
    value.zeroize();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secrets_are_redacted_in_debug_and_display() {
        // covers: FR-PRV-004, docs/10 §8.2
        let secret = ApiKeySecret::new(
            "gsk_0192b6c0a1e47c3f9d2a7b5e8f1c0d3a.secrettosecrettosecrettosecrettosecret1234",
        );
        assert_eq!(format!("{secret:?}"), "ApiKeySecret(<redacted>)");
        assert_eq!(format!("{secret}"), "<redacted>");

        let token = AccessToken::new("eyJhbGciOiJFZERTQSJ9.eyJzdWIiOiJzdWJfMSJ9.signature");
        assert_eq!(format!("{token:?}"), "AccessToken(<redacted>)");
        assert_eq!(format!("{token}"), "<redacted>");

        let ticket = Ticket::new("ticket-value");
        assert_eq!(format!("{ticket:?}"), "Ticket(<redacted>)");

        let invitation = InvitationCode::new("ABCD-EFGH-JKMN-PQRS");
        assert_eq!(format!("{invitation:?}"), "InvitationCode(<redacted>)");

        let webhook = WebhookSecret::new("whsec_secret");
        assert_eq!(format!("{webhook:?}"), "WebhookSecret(<redacted>)");
    }

    #[test]
    fn test_secrets_are_only_exposed_on_demand() {
        // covers: docs/10 §8.2
        let secret = ApiKeySecret::new("value");
        assert_eq!(secret.expose(), "value");
    }

    #[test]
    fn test_forget_zeroizes_in_place() {
        // covers: docs/10 §8.2
        let mut secret = [1u8, 2, 3, 4];
        forget(&mut secret);
        assert_eq!(secret, [0u8, 0, 0, 0]);

        let mut value = Zeroizing::new(7u32);
        forget(&mut value);
        assert_eq!(*value, 0);
    }
}
