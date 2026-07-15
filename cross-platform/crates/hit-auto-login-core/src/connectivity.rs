use std::{process::Stdio, time::Duration};

use async_trait::async_trait;
use reqwest::{Client, redirect::Policy};
use tokio::{process::Command, time::timeout};

use crate::workflow::{Connectivity, CoreError};

pub struct SystemConnectivityChecker {
    client: Client,
}

impl SystemConnectivityChecker {
    pub fn new() -> Result<Self, CoreError> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(6))
            .redirect(Policy::limited(5))
            .user_agent("HITAutoLogin/1.0")
            .build()
            .map_err(|_| CoreError::Connectivity("无法初始化 HTTPS 检测".into()))?;
        Ok(Self { client })
    }

    async fn ping_baidu(&self) -> bool {
        let mut command = Command::new("ping");
        #[cfg(target_os = "windows")]
        command.args(["-n", "1", "-w", "2000", "baidu.com"]);
        #[cfg(not(target_os = "windows"))]
        command.args(["-c", "1", "-W", "2", "baidu.com"]);
        command.stdout(Stdio::null()).stderr(Stdio::null());
        matches!(
            timeout(Duration::from_secs(4), command.status()).await,
            Ok(Ok(status)) if status.success()
        )
    }

    async fn https_baidu(&self) -> bool {
        let response = match self
            .client
            .get("https://www.baidu.com/robots.txt")
            .send()
            .await
        {
            Ok(response) => response,
            Err(_) => return false,
        };
        let status_ok = (200..500).contains(&response.status().as_u16());
        let host_ok = response.url().host_str().is_some_and(|host| {
            let host = host.to_ascii_lowercase();
            host == "baidu.com" || host.ends_with(".baidu.com")
        });
        status_ok && host_ok
    }
}

#[async_trait]
impl Connectivity for SystemConnectivityChecker {
    async fn is_online(&self) -> Result<bool, CoreError> {
        if self.ping_baidu().await {
            return Ok(true);
        }
        Ok(self.https_baidu().await)
    }
}
