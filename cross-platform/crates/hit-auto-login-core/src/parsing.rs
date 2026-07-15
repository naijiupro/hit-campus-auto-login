use regex::Regex;
use serde_json::Value;

use crate::workflow::CoreError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortalFields {
    pub ac_id: String,
    pub user_ip: String,
}

pub fn parse_portal_fields(html: &str) -> PortalFields {
    PortalFields {
        ac_id: input_value(html, "ac_id").unwrap_or_else(|| "27".to_owned()),
        user_ip: input_value(html, "user_ip").unwrap_or_default(),
    }
}

fn input_value(html: &str, id: &str) -> Option<String> {
    let tag_regex = Regex::new(r#"(?is)<input\b[^>]*>"#).ok()?;
    let id_regex = Regex::new(&format!(r#"(?is)\bid\s*=\s*["']{}["']"#, regex::escape(id))).ok()?;
    let value_regex = Regex::new(r#"(?is)\bvalue\s*=\s*["']([^"']*)["']"#).ok()?;
    tag_regex.find_iter(html).find_map(|candidate| {
        let tag = candidate.as_str();
        if !id_regex.is_match(tag) {
            return None;
        }
        value_regex
            .captures(tag)?
            .get(1)
            .map(|value| value.as_str().to_owned())
    })
}

pub fn parse_json_or_jsonp(input: &str) -> Result<Value, CoreError> {
    let start = input.find('{').ok_or(CoreError::InvalidPortalResponse)?;
    let end = input.rfind('}').ok_or(CoreError::InvalidPortalResponse)?;
    if start > end {
        return Err(CoreError::InvalidPortalResponse);
    }
    serde_json::from_str(&input[start..=end]).map_err(|_| CoreError::InvalidPortalResponse)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_live_fields_in_any_attribute_order() {
        let html = r#"
            <input value="27" type="hidden" id="ac_id">
            <input id='user_ip' name='user_ip' value='10.0.0.42'>
        "#;
        assert_eq!(
            parse_portal_fields(html),
            PortalFields {
                ac_id: "27".into(),
                user_ip: "10.0.0.42".into()
            }
        );
    }

    #[test]
    fn ac_id_falls_back_but_ip_does_not() {
        assert_eq!(
            parse_portal_fields("<html></html>"),
            PortalFields {
                ac_id: "27".into(),
                user_ip: String::new()
            }
        );
    }

    #[test]
    fn accepts_json_and_jsonp() {
        assert_eq!(
            parse_json_or_jsonp(r#"{"error":"ok"}"#).unwrap()["error"],
            "ok"
        );
        assert_eq!(
            parse_json_or_jsonp(r#"cb_1({"challenge":"abc"})"#).unwrap()["challenge"],
            "abc"
        );
    }

    #[test]
    fn rejects_invalid_jsonp() {
        assert!(matches!(
            parse_json_or_jsonp("callback(no-json)"),
            Err(CoreError::InvalidPortalResponse)
        ));
    }
}
