mod config;
mod connectivity;
mod coordinator;
mod crypto;
mod parsing;
mod portal;
mod workflow;

pub use config::Configuration;
pub use connectivity::SystemConnectivityChecker;
pub use coordinator::{RunGuard, RunTrigger, WorkflowCoordinator};
pub use crypto::{
    LoginParameters, custom_base64, hmac_md5, make_login_parameters, sha1_hex, xencode,
};
pub use parsing::{PortalFields, parse_json_or_jsonp, parse_portal_fields};
pub use portal::SrunPortalClient;
pub use workflow::{
    Connectivity, CoreError, PortalAuthenticator, ProgressCallback, WifiAdapter, Workflow,
    WorkflowLimits, WorkflowResult,
};

pub const DEFAULT_SSID: &str = "HIT-WLAN";
