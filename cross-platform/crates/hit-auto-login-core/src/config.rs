use serde::{Deserialize, Serialize};

use crate::DEFAULT_SSID;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Configuration {
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default = "default_ssid")]
    pub ssid: String,
    #[serde(default = "default_true")]
    pub launch_at_login: bool,
}

impl Default for Configuration {
    fn default() -> Self {
        Self {
            username: String::new(),
            password: String::new(),
            ssid: default_ssid(),
            launch_at_login: true,
        }
    }
}

impl Configuration {
    pub fn normalize(&mut self) {
        self.username = self.username.trim().to_owned();
        self.ssid = self.ssid.trim().to_owned();
        if self.ssid.is_empty() {
            self.ssid = default_ssid();
        }
    }

    pub fn credentials_present(&self) -> bool {
        !self.username.trim().is_empty() && !self.password.is_empty()
    }
}

fn default_ssid() -> String {
    DEFAULT_SSID.to_owned()
}

const fn default_true() -> bool {
    true
}
