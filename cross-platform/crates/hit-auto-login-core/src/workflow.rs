use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use thiserror::Error;
use tokio::time::{sleep, timeout};

use crate::Configuration;

pub type ProgressCallback = Arc<dyn Fn(&str) + Send + Sync>;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CoreError {
    #[error("请先填写学号和密码")]
    MissingCredentials,
    #[error("公网检测失败：{0}")]
    Connectivity(String),
    #[error("无法连接 HIT-WLAN：{0}")]
    Wifi(String),
    #[error("认证门户返回了无法解析的数据")]
    InvalidPortalResponse,
    #[error("获取认证挑战值失败：{0}")]
    Challenge(String),
    #[error("校园网认证失败：{0}")]
    Authentication(String),
    #[error("认证门户错误：{0}")]
    Portal(String),
    #[error("{0}超时")]
    Timeout(&'static str),
    #[error("认证请求已完成，但仍无法访问互联网")]
    VerificationFailed,
}

#[async_trait]
pub trait Connectivity: Send + Sync {
    async fn is_online(&self) -> Result<bool, CoreError>;
}

#[async_trait]
pub trait WifiAdapter: Send + Sync {
    async fn ensure_connected(&self, ssid: &str) -> Result<(), CoreError>;
}

#[async_trait]
pub trait PortalAuthenticator: Send + Sync {
    async fn authenticate(&self, username: &str, password: &str) -> Result<String, CoreError>;
}

#[derive(Clone, Copy, Debug)]
pub struct WorkflowLimits {
    pub connectivity_timeout: Duration,
    pub wifi_timeout: Duration,
    pub portal_timeout: Duration,
    pub verification_attempts: usize,
    pub verification_delay: Duration,
}

impl Default for WorkflowLimits {
    fn default() -> Self {
        Self {
            connectivity_timeout: Duration::from_secs(8),
            wifi_timeout: Duration::from_secs(20),
            portal_timeout: Duration::from_secs(20),
            verification_attempts: 3,
            verification_delay: Duration::from_millis(1500),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkflowResult {
    pub message: String,
}

pub struct Workflow<C, W, P> {
    connectivity: C,
    wifi: W,
    portal: P,
    limits: WorkflowLimits,
}

impl<C, W, P> Workflow<C, W, P>
where
    C: Connectivity,
    W: WifiAdapter,
    P: PortalAuthenticator,
{
    pub fn new(connectivity: C, wifi: W, portal: P) -> Self {
        Self {
            connectivity,
            wifi,
            portal,
            limits: WorkflowLimits::default(),
        }
    }

    pub fn with_limits(mut self, limits: WorkflowLimits) -> Self {
        self.limits = limits;
        self
    }

    pub async fn execute(
        &self,
        configuration: &Configuration,
        progress: ProgressCallback,
    ) -> Result<WorkflowResult, CoreError> {
        if !configuration.credentials_present() {
            return Err(CoreError::MissingCredentials);
        }

        progress("正在检测互联网连接…");
        if self.online().await? {
            return Ok(WorkflowResult {
                message: "网络已经连通，无需重复认证".into(),
            });
        }

        progress("正在检查并连接 Wi-Fi…");
        timeout(
            self.limits.wifi_timeout,
            self.wifi.ensure_connected(&configuration.ssid),
        )
        .await
        .map_err(|_| CoreError::Timeout("Wi-Fi 连接"))??;

        progress("Wi-Fi 已就绪，正在检测互联网…");
        if self.online().await? {
            return Ok(WorkflowResult {
                message: "网络已经连通，无需重复认证".into(),
            });
        }

        progress("正在向 HIT 门户认证…");
        let portal_message = timeout(
            self.limits.portal_timeout,
            self.portal
                .authenticate(&configuration.username, &configuration.password),
        )
        .await
        .map_err(|_| CoreError::Timeout("校园网认证"))??;

        progress("认证完成，正在验证网络…");
        for attempt in 0..self.limits.verification_attempts {
            if attempt > 0 {
                sleep(self.limits.verification_delay).await;
            }
            if self.online().await? {
                let suffix = if portal_message.is_empty() {
                    String::new()
                } else {
                    format!("（{portal_message}）")
                };
                return Ok(WorkflowResult {
                    message: format!("认证成功，网络已连通{suffix}"),
                });
            }
        }
        Err(CoreError::VerificationFailed)
    }

    async fn online(&self) -> Result<bool, CoreError> {
        timeout(
            self.limits.connectivity_timeout,
            self.connectivity.is_online(),
        )
        .await
        .map_err(|_| CoreError::Timeout("公网检测"))?
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    use super::*;

    struct SequenceConnectivity(Mutex<Vec<bool>>);
    #[async_trait]
    impl Connectivity for SequenceConnectivity {
        async fn is_online(&self) -> Result<bool, CoreError> {
            Ok(self.0.lock().unwrap().remove(0))
        }
    }

    struct CountingWifi(Arc<AtomicUsize>);
    #[async_trait]
    impl WifiAdapter for CountingWifi {
        async fn ensure_connected(&self, _: &str) -> Result<(), CoreError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    struct CountingPortal(Arc<AtomicUsize>);
    #[async_trait]
    impl PortalAuthenticator for CountingPortal {
        async fn authenticate(&self, _: &str, _: &str) -> Result<String, CoreError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(String::new())
        }
    }

    fn config() -> Configuration {
        Configuration {
            username: "2024000000".into(),
            password: "test-only".into(),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn online_skips_wifi_and_portal() {
        let wifi_calls = Arc::new(AtomicUsize::new(0));
        let portal_calls = Arc::new(AtomicUsize::new(0));
        let workflow = Workflow::new(
            SequenceConnectivity(Mutex::new(vec![true])),
            CountingWifi(wifi_calls.clone()),
            CountingPortal(portal_calls.clone()),
        );
        let result = workflow.execute(&config(), Arc::new(|_| {})).await.unwrap();
        assert_eq!(result.message, "网络已经连通，无需重复认证");
        assert_eq!(wifi_calls.load(Ordering::SeqCst), 0);
        assert_eq!(portal_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn offline_connects_authenticates_and_verifies() {
        let wifi_calls = Arc::new(AtomicUsize::new(0));
        let portal_calls = Arc::new(AtomicUsize::new(0));
        let workflow = Workflow::new(
            SequenceConnectivity(Mutex::new(vec![false, false, true])),
            CountingWifi(wifi_calls.clone()),
            CountingPortal(portal_calls.clone()),
        );
        assert!(
            workflow
                .execute(&config(), Arc::new(|_| {}))
                .await
                .unwrap()
                .message
                .starts_with("认证成功")
        );
        assert_eq!(wifi_calls.load(Ordering::SeqCst), 1);
        assert_eq!(portal_calls.load(Ordering::SeqCst), 1);
    }

    struct SlowConnectivity;
    #[async_trait]
    impl Connectivity for SlowConnectivity {
        async fn is_online(&self) -> Result<bool, CoreError> {
            sleep(Duration::from_secs(1)).await;
            Ok(false)
        }
    }

    #[tokio::test]
    async fn connectivity_timeout_is_finite_and_clear() {
        let workflow = Workflow::new(
            SlowConnectivity,
            CountingWifi(Arc::new(AtomicUsize::new(0))),
            CountingPortal(Arc::new(AtomicUsize::new(0))),
        )
        .with_limits(WorkflowLimits {
            connectivity_timeout: Duration::from_millis(10),
            ..WorkflowLimits::default()
        });
        assert_eq!(
            workflow
                .execute(&config(), Arc::new(|_| {}))
                .await
                .unwrap_err(),
            CoreError::Timeout("公网检测")
        );
    }

    struct ErrorPortal;
    #[async_trait]
    impl PortalAuthenticator for ErrorPortal {
        async fn authenticate(&self, _: &str, _: &str) -> Result<String, CoreError> {
            Err(CoreError::Authentication("账号或密码错误".into()))
        }
    }

    #[tokio::test]
    async fn portal_error_is_user_friendly() {
        let workflow = Workflow::new(
            SequenceConnectivity(Mutex::new(vec![false, false])),
            CountingWifi(Arc::new(AtomicUsize::new(0))),
            ErrorPortal,
        );
        assert_eq!(
            workflow
                .execute(&config(), Arc::new(|_| {}))
                .await
                .unwrap_err()
                .to_string(),
            "校园网认证失败：账号或密码错误"
        );
    }

    struct SlowWifi;
    #[async_trait]
    impl WifiAdapter for SlowWifi {
        async fn ensure_connected(&self, _: &str) -> Result<(), CoreError> {
            sleep(Duration::from_secs(1)).await;
            Ok(())
        }
    }

    #[tokio::test]
    async fn wifi_timeout_is_finite_and_clear() {
        let workflow = Workflow::new(
            SequenceConnectivity(Mutex::new(vec![false])),
            SlowWifi,
            CountingPortal(Arc::new(AtomicUsize::new(0))),
        )
        .with_limits(WorkflowLimits {
            wifi_timeout: Duration::from_millis(10),
            ..WorkflowLimits::default()
        });
        assert_eq!(
            workflow
                .execute(&config(), Arc::new(|_| {}))
                .await
                .unwrap_err(),
            CoreError::Timeout("Wi-Fi 连接")
        );
    }

    struct SlowPortal;
    #[async_trait]
    impl PortalAuthenticator for SlowPortal {
        async fn authenticate(&self, _: &str, _: &str) -> Result<String, CoreError> {
            sleep(Duration::from_secs(1)).await;
            Ok(String::new())
        }
    }

    #[tokio::test]
    async fn portal_timeout_is_finite_and_clear() {
        let workflow = Workflow::new(
            SequenceConnectivity(Mutex::new(vec![false, false])),
            CountingWifi(Arc::new(AtomicUsize::new(0))),
            SlowPortal,
        )
        .with_limits(WorkflowLimits {
            portal_timeout: Duration::from_millis(10),
            ..WorkflowLimits::default()
        });
        assert_eq!(
            workflow
                .execute(&config(), Arc::new(|_| {}))
                .await
                .unwrap_err(),
            CoreError::Timeout("校园网认证")
        );
    }
}
