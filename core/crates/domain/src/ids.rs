//! Opaque string ids for Providers, Provider Connections and Plugins.

use std::fmt;

use serde::{Deserialize, Serialize};

macro_rules! string_id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Wraps an id. Ids are opaque: Ristle never parses them.
            pub fn new(id: impl Into<String>) -> Self {
                Self(id.into())
            }

            /// The id as it was given.
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// Whether the id is the empty string, which never names anything.
            pub fn is_empty(&self) -> bool {
                self.0.is_empty()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<String> for $name {
            fn from(id: String) -> Self {
                Self(id)
            }
        }

        impl From<&str> for $name {
            fn from(id: &str) -> Self {
                Self(id.to_owned())
            }
        }
    };
}

string_id!(
    /// A Provider, named by its Provider Plugin's manifest id, such as
    /// `spotify` or `soundcloud`.
    ProviderId
);

string_id!(
    /// One Provider Connection: a user's configured access to a Provider.
    ConnectionId
);

string_id!(
    /// An installed Plugin, by its manifest id.
    PluginId
);
