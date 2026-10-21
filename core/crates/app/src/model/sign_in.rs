//! Authorization Sessions as UIs drive them.
//!
//! The core owns the session: OAuth state, PKCE, callback validation and
//! polling. A UI only does what needs the platform (open a URL, show a web
//! view, a form, a folder picker) and answers.

use super::plugins::ConnectionView;

/// How a sign-in URL is opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Opener {
    /// The system browser or a browser tab (Custom Tabs,
    /// `ASWebAuthenticationSession`).
    Browser,
    /// The Provider's app, through a verified app link, falling back to the
    /// browser.
    App,
}

/// The kind of a form field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FieldKind {
    /// Plain text.
    Text,
    /// Hidden while typed.
    Secret,
    /// An email address.
    Email,
    /// A URL.
    Url,
}

/// One field of a sign-in form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormField {
    /// The name the answer uses.
    pub name: String,
    /// The label.
    pub label: String,
    /// The kind.
    pub kind: FieldKind,
    /// Must not be empty.
    pub required: bool,
    /// Shown while empty.
    pub placeholder: Option<String>,
    /// Pre-filled value.
    pub value: Option<String>,
}

/// A sign-in form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignInForm {
    /// The title.
    pub title: String,
    /// Instructions above the fields.
    pub instructions: Option<String>,
    /// The fields.
    pub fields: Vec<FormField>,
    /// A help link. HTTPS only.
    pub help_url: Option<String>,
}

/// The user agent of a sign-in web view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserAgent {
    /// The platform web view's own.
    Platform,
    /// A desktop browser's.
    Desktop,
    /// This exact string.
    Custom {
        /// The user agent.
        value: String,
    },
}

/// Where a captured value is read in a sign-in web view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureSource {
    /// A cookie with this name.
    Cookie {
        /// The cookie name.
        name: String,
    },
    /// A request header with this name.
    RequestHeader {
        /// The header name.
        name: String,
    },
    /// A response header with this name.
    ResponseHeader {
        /// The header name.
        name: String,
    },
    /// A query parameter of a request whose URL starts with a prefix.
    QueryParam {
        /// The URL prefix.
        url_prefix: String,
        /// The parameter name.
        name: String,
    },
    /// A value a page script reports.
    Script,
}

/// One value a sign-in web view captures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureRule {
    /// The name the answer uses.
    pub name: String,
    /// Where to read it, first match wins.
    pub sources: Vec<CaptureSource>,
    /// The capture is complete only once this value is found.
    pub required: bool,
}

/// A script injected into pages whose URL starts with a prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageScript {
    /// The URL prefix.
    pub url_prefix: String,
    /// The JavaScript.
    pub code: String,
}

/// A sign-in web view with capture rules. The core has checked every URL
/// and host against the Plugin's manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebCapture {
    /// The first page.
    pub start_url: String,
    /// The user agent.
    pub user_agent: UserAgent,
    /// Hosts whose traffic is observed.
    pub observe: Vec<String>,
    /// Values to capture.
    pub captures: Vec<CaptureRule>,
    /// Scripts to inject.
    pub scripts: Vec<PageScript>,
}

/// A self-hosted server, such as Jellyfin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaServer {
    /// The server's own name.
    pub name: String,
    /// Base URLs that reach it. All of them are granted together.
    pub urls: Vec<String>,
}

/// A folder the user picked with the system folder picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickedFolder {
    /// The platform's persistent reference: a tree URI on Android, a
    /// security-scoped bookmark (base64) on Apple platforms.
    pub uri: String,
    /// The folder's name as the system shows it.
    pub name: String,
}

/// A name and a value from a form or a web capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedValue {
    /// The field or capture name.
    pub name: String,
    /// The value.
    pub value: String,
}

/// The next thing a sign-in needs from the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignInStep {
    /// Open a URL. The callback arrives as a link to the app; answer with
    /// [`SignInAnswer::Callback`].
    OpenUrl {
        /// The Authorization Session.
        session: String,
        /// The URL.
        url: String,
        /// How to open it.
        opener: Opener,
        /// Where to go when the app is not installed.
        fallback_url: Option<String>,
    },
    /// Show a form; answer with [`SignInAnswer::Form`].
    Form {
        /// The Authorization Session.
        session: String,
        /// The form.
        form: SignInForm,
    },
    /// Show a web view with capture rules; answer with
    /// [`SignInAnswer::WebCapture`].
    WebCapture {
        /// The Authorization Session.
        session: String,
        /// The web view's rules.
        capture: WebCapture,
    },
    /// Show a code to enter on another device; answer with
    /// [`SignInAnswer::Continue`], which returns once the user has entered it.
    DeviceCode {
        /// The Authorization Session.
        session: String,
        /// Where to enter the code.
        verification_url: String,
        /// The code.
        user_code: String,
        /// When the code expires, in milliseconds since the Unix epoch.
        expires_at_ms: u64,
    },
    /// Let the user choose a server; answer with [`SignInAnswer::Server`].
    ChooseServer {
        /// The Authorization Session.
        session: String,
        /// Servers found on the local network.
        found: Vec<MediaServer>,
        /// The user may type an address.
        allow_entry: bool,
        /// Shown in the address field while empty.
        placeholder: Option<String>,
    },
    /// Let the user pick music folders with the system picker; answer with
    /// [`SignInAnswer::Folders`].
    ChooseFolders {
        /// The Authorization Session.
        session: String,
        /// Shown with the picker.
        prompt: Option<String>,
        /// More than one folder may be picked.
        multiple: bool,
    },
    /// Signed in.
    Done {
        /// The new or re-authorized Provider Connection.
        connection: ConnectionView,
    },
}

/// The user's answer to a [`SignInStep`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignInAnswer {
    /// The link the app was opened with after `OpenUrl`.
    Callback {
        /// The full URL.
        url: String,
    },
    /// The filled form.
    Form {
        /// One value per field.
        values: Vec<NamedValue>,
    },
    /// What the web view captured.
    WebCapture {
        /// One value per capture rule found.
        values: Vec<NamedValue>,
    },
    /// Go on after `DeviceCode`.
    Continue,
    /// The chosen or typed server.
    Server {
        /// The server.
        server: MediaServer,
    },
    /// The picked folders.
    Folders {
        /// The folders.
        folders: Vec<PickedFolder>,
    },
}
