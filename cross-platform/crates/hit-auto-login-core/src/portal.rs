use std::{
    sync::OnceLock,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use regex::Regex;
use reqwest::{
    Client, StatusCode, Url,
    header::{ACCEPT, ACCEPT_LANGUAGE, HeaderMap, HeaderValue, REFERER, USER_AGENT},
    redirect::Policy,
};
use serde_json::Value;
use url::form_urlencoded::Serializer;

use crate::{
    crypto::make_login_parameters,
    parsing::{parse_json_or_jsonp, parse_portal_fields},
    workflow::{CoreError, PortalAuthenticator},
};

const PORTAL_BASE: &str = "https://wp.hit.edu.cn";

/// Encodes every name and value independently using HTML form query rules.
/// In particular, a literal `+` becomes `%2B` rather than being interpreted as a space.
pub struct FormQueryEncoder;

impl FormQueryEncoder {
    pub fn encode<'a>(pairs: impl IntoIterator<Item = (&'a str, &'a str)>) -> String {
        let mut serializer = Serializer::new(String::new());
        for (name, value) in pairs {
            serializer.append_pair(name, value);
        }
        serializer.finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PortalInterpretation {
    Success(String),
    AlreadyOnline,
    Failure(String),
}

pub struct PortalResponseInterpreter;

impl PortalResponseInterpreter {
    pub fn interpret(response: &Value) -> PortalInterpretation {
        let error = meaningful_field(response, "error").unwrap_or_default();
        if error == "ok" {
            let message = first_meaningful(response, &["ploy_msg", "suc_msg"])
                .filter(|text| !text.starts_with("E0000"))
                .map(safe_portal_message)
                .unwrap_or_default();
            return PortalInterpretation::Success(message);
        }

        let candidates = ["ploy_msg", "error_msg", "ecode", "error", "res"];
        if candidates.iter().any(|key| {
            meaningful_field(response, key)
                .is_some_and(|text| text.to_ascii_lowercase().contains("already_online"))
        }) {
            return PortalInterpretation::AlreadyOnline;
        }

        for key in candidates {
            let Some(message) = meaningful_field(response, key) else {
                continue;
            };
            if is_generic_failure_marker(&message) {
                continue;
            }
            if let Some(translated) = translate_error_code(&message) {
                return PortalInterpretation::Failure(translated);
            }
            return PortalInterpretation::Failure(safe_portal_message(message));
        }

        let error = value_string(response, "error");
        let detail = if error.is_empty() || error == "0" {
            "认证服务器拒绝登录请求".to_owned()
        } else {
            format!("认证服务器拒绝登录请求（{}）", safe_portal_message(error))
        };
        PortalInterpretation::Failure(detail)
    }
}

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
            .default_headers(compatibility_headers(os)?)
            .cookie_store(true)
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
            .get(format!("{PORTAL_BASE}/"))
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
        let url = build_portal_url(path, &query)?;

        // Never propagate reqwest's error text here: it may contain the complete auth URL.
        let response = self
            .client
            .get(url)
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
            let message = first_meaningful(&challenge, &["error_msg", "ecode", "error"])
                .and_then(|text| translate_error_code(&text).or(Some(text)))
                .unwrap_or_else(|| "认证服务器未返回有效 challenge".into());
            return Err(CoreError::Challenge(safe_portal_message(message)));
        }

        let client_ip = select_client_ip(&challenge, &fields.user_ip)
            .ok_or_else(|| CoreError::Portal("认证门户未返回客户端 IP".into()))?;
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

        match PortalResponseInterpreter::interpret(&response) {
            PortalInterpretation::Success(message) => Ok(message),
            PortalInterpretation::AlreadyOnline => Ok("当前 IP 已在线".into()),
            PortalInterpretation::Failure(message) => Err(CoreError::Authentication(message)),
        }
    }
}

fn build_portal_url(path: &str, pairs: &[(&str, &str)]) -> Result<Url, CoreError> {
    let encoded = FormQueryEncoder::encode(pairs.iter().copied());
    Url::parse(&format!("{PORTAL_BASE}{path}?{encoded}"))
        .map_err(|_| CoreError::InvalidPortalResponse)
}

fn compatibility_headers(os: &str) -> Result<HeaderMap, CoreError> {
    let platform = if os.eq_ignore_ascii_case("Windows") {
        "Windows NT 10.0; Win64; x64"
    } else {
        "X11; Linux x86_64"
    };
    let user_agent = format!(
        "Mozilla/5.0 ({platform}) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0 Safari/537.36 HITAutoLogin/{}",
        env!("CARGO_PKG_VERSION")
    );
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("*/*"));
    headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("zh-CN,zh;q=0.9"));
    headers.insert(REFERER, HeaderValue::from_static("https://wp.hit.edu.cn/"));
    headers.insert(
        USER_AGENT,
        HeaderValue::from_str(&user_agent)
            .map_err(|_| CoreError::Portal("无法初始化请求头".into()))?,
    );
    Ok(headers)
}

fn select_client_ip(challenge: &Value, portal_ip: &str) -> Option<String> {
    meaningful_field(challenge, "client_ip").or_else(|| {
        let portal_ip = portal_ip.trim();
        (!portal_ip.is_empty()).then(|| portal_ip.to_owned())
    })
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

fn meaningful_field(value: &Value, key: &str) -> Option<String> {
    let text = value_string(value, key);
    let trimmed = text.trim();
    (!trimmed.is_empty() && trimmed != "0").then(|| trimmed.to_owned())
}

fn first_meaningful(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| meaningful_field(value, key))
}

fn is_generic_failure_marker(message: &str) -> bool {
    matches!(
        message.to_ascii_lowercase().as_str(),
        "fail" | "login_error"
    )
}

fn translate_error_code(message: &str) -> Option<String> {
    let code = message.trim().to_ascii_uppercase();
    let explanation = match code.as_str() {
        "E2531" => "用户不存在或账号信息有误",
        "E2532" => "两次认证间隔太短，请等待 10 秒后重试",
        "E2533" => "密码错误次数超过限制，请等待 5 分钟后重试",
        "E2553" => "账号或密码错误",
        "E2606" => "用户账号已暂停",
        "E2614" => "MAC 地址绑定错误",
        "E2615" => "IP 地址绑定错误",
        "E2616" => "账号余额不足",
        "E2620" => "登录设备数量已达到上限",
        "E2833" => "当前 IP 地址有误，请重新连接 Wi-Fi",
        "E6506" => "用户名或密码错误",
        "E6515" => "系统禁止客户端登录，需要使用网页认证",
        "E6529" => "认证失败，但服务器没有返回详细原因",
        _ => return None,
    };
    Some(format!("{explanation}（{code}）"))
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
    static SENSITIVE_ASSIGNMENT: OnceLock<Regex> = OnceLock::new();
    let pattern = SENSITIVE_ASSIGNMENT.get_or_init(|| {
        Regex::new(r"(?i)\b(username|password|challenge|info|chksum)\s*[:=]\s*[^&\s,;]+")
            .expect("static sensitive-field regex is valid")
    });
    cleaned = pattern.replace_all(&cleaned, "$1=<已隐藏>").into_owned();
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
    use serde_json::json;

    #[test]
    fn form_query_encodes_srun_info_exactly() {
        let encoded = FormQueryEncoder::encode([("info", "{SRBX1}a+b/c==")]);
        assert_eq!(encoded, "info=%7BSRBX1%7Da%2Bb%2Fc%3D%3D");
        assert!(!encoded.contains('+'));
    }

    #[test]
    fn login_url_is_encoded_once_and_contains_no_raw_plus() {
        let url = build_portal_url(
            "/cgi-bin/srun_portal",
            &[
                ("password", "{MD5}abc+def"),
                ("info", "{SRBX1}a+b/c=="),
                ("chksum", "123abc"),
            ],
        )
        .unwrap();
        let query = url.query().unwrap();
        assert!(!query.contains('+'));
        assert!(query.contains("password=%7BMD5%7Dabc%2Bdef"));
        assert!(query.contains("info=%7BSRBX1%7Da%2Bb%2Fc%3D%3D"));
        assert!(!query.contains("%252B"));
        let decoded = url::form_urlencoded::parse(query.as_bytes()).collect::<Vec<_>>();
        assert!(decoded.contains(&("info".into(), "{SRBX1}a+b/c==".into())));
    }

    #[test]
    fn challenge_client_ip_has_priority_over_portal_html() {
        let challenge = json!({"client_ip": "10.0.0.99"});
        assert_eq!(
            select_client_ip(&challenge, "10.0.0.42"),
            Some("10.0.0.99".into())
        );
    }

    #[test]
    fn portal_ip_is_used_only_as_fallback() {
        assert_eq!(
            select_client_ip(&json!({"client_ip": ""}), "10.0.0.42"),
            Some("10.0.0.42".into())
        );
    }

    #[test]
    fn zero_fields_do_not_become_the_error_message() {
        let response = json!({
            "error": "login_error",
            "error_msg": 0,
            "ecode": 0,
            "res": "fail"
        });
        assert_eq!(
            PortalResponseInterpreter::interpret(&response),
            PortalInterpretation::Failure("认证服务器拒绝登录请求（login_error）".into())
        );
    }

    #[test]
    fn known_error_code_is_translated() {
        let response = json!({"error": "login_error", "ecode": "E2553"});
        assert_eq!(
            PortalResponseInterpreter::interpret(&response),
            PortalInterpretation::Failure("账号或密码错误（E2553）".into())
        );
    }

    #[test]
    fn error_ok_wins_over_zero_ecode() {
        let response = json!({"error": "ok", "ecode": 0, "suc_msg": "登录成功"});
        assert_eq!(
            PortalResponseInterpreter::interpret(&response),
            PortalInterpretation::Success("登录成功".into())
        );
    }

    #[test]
    fn compatibility_headers_match_portal_expectations() {
        let headers = compatibility_headers("Linux").unwrap();
        assert_eq!(headers[ACCEPT], "*/*");
        assert_eq!(headers[ACCEPT_LANGUAGE], "zh-CN,zh;q=0.9");
        assert_eq!(headers[REFERER], "https://wp.hit.edu.cn/");
        assert!(
            headers[USER_AGENT]
                .to_str()
                .unwrap()
                .contains("X11; Linux x86_64")
        );
    }

    #[test]
    fn portal_message_does_not_keep_urls() {
        let output =
            safe_portal_message("failed https://wp.hit.edu.cn/?password=x&info=y next".into());
        assert_eq!(output, "failed <已隐藏 URL> next");
    }

    #[test]
    fn portal_message_redacts_sensitive_assignments() {
        let output = safe_portal_message(
            "password=secret challenge:token info={SRBX1}abc chksum=deadbeef username=2024000000"
                .into(),
        );
        for secret in ["secret", "token", "{SRBX1}abc", "deadbeef", "2024000000"] {
            assert!(!output.contains(secret));
        }
    }
}
