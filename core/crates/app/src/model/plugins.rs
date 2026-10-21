//! Plugins, Provider Connections, installation and Plugin settings.

use super::items::ArtworkImage;

/// The state of a Provider Connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
    /// Signed in and usable.
    Ready,
    /// The Provider rejected the credential; sign in again.
    NeedsSignIn,
    /// The Plugin is disabled or cannot run.
    Unavailable {
        /// Why.
        reason: String,
    },
}

/// A Provider Connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionView {
    /// The connection's id.
    pub id: String,
    /// The Plugin it belongs to.
    pub plugin: String,
    /// The account name, or the server or folder name.
    pub name: String,
    /// The account's avatar.
    pub avatar: Vec<ArtworkImage>,
    /// Whether it is usable.
    pub state: ConnectionState,
}

/// An installed Plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginView {
    /// The manifest id.
    pub id: String,
    /// The display name.
    pub name: String,
    /// The installed version.
    pub version: String,
    /// The Publisher's key id.
    pub publisher: String,
    /// Whether it is enabled.
    pub enabled: bool,
    /// Whether the Plugin can sign in (`begin_sign_in`).
    pub has_accounts: bool,
    /// Its Provider Connections.
    pub connections: Vec<ConnectionView>,
    /// A newer version the update feed offers.
    pub update: Option<String>,
}

/// What installing a reviewed package would do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallKind {
    /// A Plugin that is not installed.
    New,
    /// A newer version by the same Publisher. `permissions` lists only what
    /// it adds; when that is empty it needs no consent screen.
    Update {
        /// The installed version.
        from_version: String,
    },
    /// An older version by the same Publisher, installed by hand.
    Downgrade {
        /// The installed version.
        from_version: String,
    },
    /// The same Plugin id signed by another Publisher. Shown as a warned
    /// takeover, never as an update.
    Takeover {
        /// The installed Publisher's key id.
        from_publisher: String,
    },
}

/// One line of the consent screen. UIs word them (packaging.md,
/// "Permissions and consent").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Permission {
    /// Connects to these hosts.
    ConnectsTo {
        /// The hosts.
        hosts: Vec<String>,
    },
    /// Checks for updates at this host.
    ChecksForUpdates {
        /// The host.
        host: String,
    },
    /// Shares item details with Party guests.
    SharesWithPartyGuests {
        /// The shared fields, empty when every field is shared.
        fields: Vec<String>,
    },
    /// Stores more than the default 5 MiB on the device.
    StoresUpTo {
        /// The limit in MiB.
        mib: u32,
    },
    /// Connects to servers the user chooses, including on the home network.
    ChosenServers,
    /// Reads music folders the user chooses.
    ChosenFolders,
    /// Opens direct, non-HTTPS connections.
    OpensSockets {
        /// `host:port` targets.
        targets: Vec<String>,
    },
    /// Watches pages and traffic of these sites inside its sign-in window.
    ObservesSignInSites {
        /// The hosts.
        hosts: Vec<String>,
    },
    /// Opens these apps to sign in.
    OpensApps {
        /// The URL schemes, or the apps they resolve to.
        apps: Vec<String>,
    },
    /// Signs in as the user and can do anything their account can.
    SignsIn,
    /// Can like, save and follow.
    Likes,
    /// Can repost.
    Reposts,
    /// Can create, change and delete playlists.
    EditsPlaylists,
    /// Shows Ristle in the service's own apps and takes commands from them.
    ProjectsQueue,
}

/// A reviewed Plugin Package, ready to install with the user's consent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallReview {
    /// Identifies this review for `install`.
    pub review: String,
    /// The manifest id.
    pub id: String,
    /// The display name.
    pub name: String,
    /// The package's version.
    pub version: String,
    /// The signing Publisher's key id.
    pub publisher: String,
    /// New install, update, downgrade or takeover.
    pub kind: InstallKind,
    /// The consent lines; for an update, only the new ones.
    pub permissions: Vec<Permission>,
    /// The Plugin's icon, decoded and checked by the core (PNG).
    pub icon: Option<Vec<u8>>,
}

/// The user's answer to an install review. It must match the review's
/// [`InstallKind`]: a takeover or downgrade is never accepted by `Install`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Consent {
    /// Install a new Plugin or an update.
    Install,
    /// Install an older version.
    Downgrade,
    /// Replace the installed Publisher.
    Takeover,
}

/// Who a Plugin setting applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SettingScope {
    /// Every Provider Connection of the Plugin.
    Plugin,
    /// Only this Provider Connection.
    Connection,
}

/// One option of a choice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceOption {
    /// The value stored.
    pub value: String,
    /// The label shown.
    pub label: String,
}

/// The control of one setting, with its current value.
#[derive(Debug, Clone, PartialEq)]
pub enum SettingControl {
    /// On or off.
    Toggle {
        /// The current value.
        value: bool,
    },
    /// One of several options.
    Choice {
        /// The options.
        options: Vec<ChoiceOption>,
        /// The selected option's value.
        value: String,
    },
    /// Free text.
    Text {
        /// Hide what is typed.
        secret: bool,
        /// Shown while empty.
        placeholder: Option<String>,
        /// The current value.
        value: String,
    },
    /// A number in a range.
    Number {
        /// Lowest value.
        min: f64,
        /// Highest value.
        max: f64,
        /// Step between values.
        step: f64,
        /// Unit shown after the value.
        unit: Option<String>,
        /// The current value.
        value: f64,
    },
    /// An action.
    Button {
        /// A confirmation to show before pressing.
        confirm: Option<String>,
    },
}

/// One Plugin setting.
#[derive(Debug, Clone, PartialEq)]
pub struct Setting {
    /// The key `apply_setting` takes.
    pub key: String,
    /// The title.
    pub title: String,
    /// A longer explanation.
    pub description: Option<String>,
    /// Who it applies to.
    pub scope: SettingScope,
    /// The control and its value.
    pub control: SettingControl,
}

/// A group of Plugin settings.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingsGroup {
    /// The heading.
    pub title: Option<String>,
    /// The settings, in order.
    pub settings: Vec<Setting>,
}

/// A new value for a Plugin setting.
#[derive(Debug, Clone, PartialEq)]
pub enum SettingValue {
    /// For a toggle.
    Toggle {
        /// The value.
        value: bool,
    },
    /// For a choice: an option's value.
    Choice {
        /// The value.
        value: String,
    },
    /// For text.
    Text {
        /// The value.
        value: String,
    },
    /// For a number.
    Number {
        /// The value.
        value: f64,
    },
    /// A button was pressed.
    Pressed,
}
