use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use reqwest::{Client, StatusCode, redirect::Policy};
use serde_json::Value;

use crate::{
    crypto::make_login_parameters,
    parsing::{parse_json_or_jsonp, parse_portal_fields},
    workflow::{CoreError, PortalAuthenticator},
};

pub struct SrunPortalClient {
    client: Client,
    os: &'static str,
    device_name: &'static str,
}

impl SrunPortalClient {
    pub fn new(os: &'static str, device_name: &'static str) -> Result<Self, CoreError> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(4))
            .timeout(Duration::from_secs(12))
            .redirect(Policy::limited(5))
            .user_agent("HITAutoLogin/1.0")
            .build()
            .map_err(|_| CoreError::Portal("无法初始化认证客户端".into()))?;
        Ok(Self {
            client,
            os,
            device_name,
        })
    }

    async fn load_fields(&self) -> Result<crate::PortalFields, CoreError> {
        let response = self
            .client
            .get("https://wp.hit.edu.cn/")
            .send()
            .await
            .map_err(|_| CoreError::Portal("无法访问认证门户首页".into()))?;
        if !success_or_redirect(response.status()) {
            return Err(CoreError::Portal("认证门户首页返回异常状态".into()));
        }
        let html = response
            .text()
            .await
            .map_err(|_| CoreError::Portal("无法读取认证门户首页".into()))?;
        Ok(parse_portal_fields(&html))
    }

    async fn request_jsonp(&self, path: &str, pairs: &[(&str, &str)]) -> Result<Value, CoreError> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .to_string();
        let callback = format!("HITAutoLogin_{timestamp}");
        let mut query = Vec::with_capacity(pairs.len() + 2);
        query.push(("callback", callback.as_str()));
        query.extend_from_slice(pairs);
        query.push(("_", timestamp.as_str()));

        // Never propagate reqwest's error text here: it may contain the complete auth URL.
        let response = self
            .client
            .get(format!("https://wp.hit.edu.cn{path}"))
            .query(&query)
            .send()
            .await
            .map_err(|_| CoreError::Portal("认证接口请求超时或连接失败".into()))?;
        if !success_or_redirect(response.status()) {
            return Err(CoreError::Portal("认证接口返回异常状态".into()));
        }
        let body = response
            .text()
            .await
            .map_err(|_| CoreError::InvalidPortalResponse)?;
        parse_json_or_jsonp(&body)
    }
}

#[async_trait]
impl PortalAuthenticator for SrunPortalClient {
    async fn authenticate(&self, username: &str, password: &str) -> Result<String, CoreError> {
        let fields = self.load_fields().await?;
        let challenge = self
            .request_jsonp(
                "/cgi-bin/get_challenge",
                &[("username", username), ("ip", fields.user_ip.as_str())],
            )
            .await?;

        let challenge_error = value_string(&challenge, "error");
        let token = value_string(&challenge, "challenge");
        if challenge_error != "ok" || token.is_empty() {
            return Err(CoreError::Challenge(safe_portal_message(first_nonempty(
                &challenge,
                &["error_msg", "ecode", "error"],
            ))));
        }

        let client_ip = if fields.user_ip.is_empty() {
            value_string(&challenge, "client_ip")
        } else {
            fields.user_ip
        };
        if client_ip.is_empty() {
            return Err(CoreError::Portal("认证门户未返回客户端 IP".into()));
        }

        let parameters = make_login_parameters(
            username,
            password,
            &client_ip,
            &fields.ac_id,
            &token,
            self.os,
            self.device_name,
        );
        let response = self
            .request_jsonp("/cgi-bin/srun_portal", &parameters.query_pairs())
            .await?;

        let error = value_string(&response, "error");
        let success_message = first_nonempty(&response, &["ploy_msg", "suc_msg"]);
        if error == "ok" {
            if success_message.starts_with("E0000") {
                return Ok(String::new());
            }
            return Ok(safe_portal_message(success_message));
        }

        let message = first_nonempty(
            &response,
            &["ploy_msg", "error_msg", "suc_msg", "ecode", "error"],
        );
        if message.to_ascii_lowercase().contains("already_online") {
            return Ok("当前 IP 已在线".into());
        }
        Err(CoreError::Authentication(safe_portal_message(message)))
    }
}

fn success_or_redirect(status: StatusCode) -> bool {
    (200..400).contains(&status.as_u16())
}

fn value_string(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(|item| match item {
            Value::String(text) => Some(text.clone()),
            Value::Number(number) => Some(number.to_string()),
            _ => None,
        })
        .unwrap_or_default()
}

fn first_nonempty(value: &Value, keys: &[&str]) -> String {
    keys.iter()
        .map(|key| value_string(value, key))
        .find(|text| !text.is_empty())
        .unwrap_or_else(|| "未知错误".into())
}

pub fn safe_portal_message(message: String) -> String {
    let mut cleaned = message.replace(['\r', '\n', '\t'], " ");
    for prefix in ["http://", "https://"] {
        while let Some(start) = cleaned.find(prefix) {
            let end = cleaned[start..]
                .find(char::is_whitespace)
                .map_or(cleaned.len(), |offset| start + offset);
            cleaned.replace_range(start..end, "<已隐藏 URL>");
        }
    }
    cleaned.truncate(cleaned.floor_char_boundary(160));
    if cleaned.trim().is_empty() {
        "未知错误".into()
    } else {
        cleaned.trim().to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portal_message_does_not_keep_urls() {
        let output =
            safe_portal_message("failed https://wp.hit.edu.cn/?password=x&info=y next".into());
        assert_eq!(output, "failed <已隐藏 URL> next");
    }
}
