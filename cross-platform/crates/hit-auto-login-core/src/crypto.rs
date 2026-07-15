use base64::{Engine, engine::general_purpose::STANDARD};
use hmac::{Hmac, Mac};
use md5::Md5;
use serde::Serialize;
use sha1::{Digest, Sha1};

const CUSTOM_BASE64_ALPHABET: &[u8; 64] =
    b"LVoJPiCN2R8G90yg+hmFHuacZ1OWMnrsSTXkYpUq/3dlbfKwv6xztjI7DeBE45QA";
const STANDARD_BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const ENC_VER: &str = "srun_bx1";
const N: &str = "200";
const TYPE: &str = "1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoginParameters {
    pub action: String,
    pub username: String,
    pub password: String,
    pub ac_id: String,
    pub ip: String,
    pub chksum: String,
    pub info: String,
    pub n: String,
    pub login_type: String,
    pub os: String,
    pub name: String,
    pub double_stack: String,
}

impl LoginParameters {
    pub fn query_pairs(&self) -> [(&str, &str); 12] {
        [
            ("action", &self.action),
            ("username", &self.username),
            ("password", &self.password),
            ("ac_id", &self.ac_id),
            ("ip", &self.ip),
            ("chksum", &self.chksum),
            ("info", &self.info),
            ("n", &self.n),
            ("type", &self.login_type),
            ("os", &self.os),
            ("name", &self.name),
            ("double_stack", &self.double_stack),
        ]
    }
}

#[derive(Serialize)]
struct Info<'a> {
    username: &'a str,
    password: &'a str,
    ip: &'a str,
    acid: &'a str,
    enc_ver: &'static str,
}

pub fn make_login_parameters(
    username: &str,
    password: &str,
    ip: &str,
    ac_id: &str,
    token: &str,
    os: &str,
    name: &str,
) -> LoginParameters {
    let hmd5 = hmac_md5(password, token);
    // Struct field order is deliberately the Srun-required JSON field order.
    let info_json = serde_json::to_string(&Info {
        username,
        password,
        ip,
        acid: ac_id,
        enc_ver: ENC_VER,
    })
    .expect("serializing string-only Srun info cannot fail");
    let info = format!("{{SRBX1}}{}", custom_base64(&xencode(&info_json, token)));
    let checksum_source = format!(
        "{token}{username}{token}{hmd5}{token}{ac_id}{token}{ip}{token}{N}{token}{TYPE}{token}{info}"
    );

    LoginParameters {
        action: "login".to_owned(),
        username: username.to_owned(),
        password: format!("{{MD5}}{hmd5}"),
        ac_id: ac_id.to_owned(),
        ip: ip.to_owned(),
        chksum: sha1_hex(&checksum_source),
        info,
        n: N.to_owned(),
        login_type: TYPE.to_owned(),
        os: os.to_owned(),
        name: name.to_owned(),
        double_stack: "0".to_owned(),
    }
}

pub fn hmac_md5(message: &str, key: &str) -> String {
    let mut mac = Hmac::<Md5>::new_from_slice(key.as_bytes()).expect("HMAC accepts every key size");
    mac.update(message.as_bytes());
    hex_lower(&mac.finalize().into_bytes())
}

pub fn sha1_hex(text: &str) -> String {
    hex_lower(&Sha1::digest(text.as_bytes()))
}

fn hex_lower(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
}

/// Srun XXTEA variant. Packing deliberately mirrors JavaScript charCodeAt over UTF-16 units.
pub fn xencode(text: &str, key: &str) -> Vec<u8> {
    if text.is_empty() {
        return Vec::new();
    }

    let mut values = packed_words(text, true);
    let mut key_words = packed_words(key, false);
    key_words.resize(4, 0);

    let last = values.len() - 1;
    let mut z = values[last];
    let delta = 0x9e37_79b9_u32;
    let mut rounds = 6 + 52 / values.len();
    let mut sum = 0_u32;

    while rounds > 0 {
        rounds -= 1;
        sum = sum.wrapping_add(delta);
        let e = (sum >> 2) & 3;

        for index in 0..last {
            let y = values[index + 1];
            let mut mixed = (z >> 5) ^ y.wrapping_shl(2);
            mixed = mixed.wrapping_add(((y >> 3) ^ z.wrapping_shl(4)) ^ (sum ^ y));
            mixed = mixed.wrapping_add(key_words[((index as u32 & 3) ^ e) as usize] ^ z);
            values[index] = values[index].wrapping_add(mixed);
            z = values[index];
        }

        let y = values[0];
        let mut mixed = (z >> 5) ^ y.wrapping_shl(2);
        mixed = mixed.wrapping_add(((y >> 3) ^ z.wrapping_shl(4)) ^ (sum ^ y));
        mixed = mixed.wrapping_add(key_words[((last as u32 & 3) ^ e) as usize] ^ z);
        values[last] = values[last].wrapping_add(mixed);
        z = values[last];
    }

    values
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>()
}

pub fn custom_base64(data: &[u8]) -> String {
    STANDARD
        .encode(data)
        .bytes()
        .map(|byte| {
            STANDARD_BASE64_ALPHABET
                .iter()
                .position(|candidate| *candidate == byte)
                .map_or(byte, |index| CUSTOM_BASE64_ALPHABET[index])
        })
        .map(char::from)
        .collect()
}

fn packed_words(text: &str, include_length: bool) -> Vec<u32> {
    let units = text.encode_utf16().collect::<Vec<_>>();
    let mut words = vec![0_u32; units.len().div_ceil(4)];
    for (index, unit) in units.iter().copied().enumerate() {
        words[index >> 2] |= u32::from(unit) << ((index & 3) * 8);
    }
    if include_length {
        words.push(units.len() as u32);
    }
    words
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hmac_md5_known_vector() {
        assert_eq!(
            hmac_md5("The quick brown fox jumps over the lazy dog", "key"),
            "80070713463e7749b90c2dc24911e275"
        );
    }

    #[test]
    fn sha1_known_vector() {
        assert_eq!(
            sha1_hex("The quick brown fox jumps over the lazy dog"),
            "2fd4e1c67a2d28fced849ee1bb76e7391b93eb12"
        );
    }

    #[test]
    fn xencode_known_vector() {
        assert_eq!(
            hex_lower(&xencode("hello", "0123456789abcdef")),
            "e1546146d6ebf3048b8be7fd"
        );
    }

    #[test]
    fn custom_base64_known_vector() {
        assert_eq!(custom_base64(b"hello"), "OCubWC4=");
    }

    #[test]
    fn complete_parameters_match_javascript_reference() {
        let parameters = make_login_parameters(
            "2024000000",
            "password",
            "10.0.0.42",
            "27",
            "0123456789abcdef0123456789abcdef",
            "Windows",
            "Windows PC",
        );
        assert_eq!(parameters.password, "{MD5}d317550f8126002512bb01d794c9ffa2");
        assert_eq!(
            parameters.info,
            "{SRBX1}W+FIdBHb99ePoYgDyr5/mdNfPyeR8eEmbRC0h0YvrwE85XNO0BKRwwKSN8I2mQNNXdv1KZ/pq6RHfcDBkEpR+do5TMf2oZZjWENME+3DjWqfsRAa1QgJVl4U60eRctwlVzkYOP0Sm6S="
        );
        assert_eq!(
            parameters.chksum,
            "923eb2c3ac4a31fd825d66a6a38d6b4a6fc79370"
        );
    }

    #[test]
    fn unicode_password_matches_javascript_reference() {
        let parameters = make_login_parameters(
            "2024000000",
            "密码🔐",
            "10.0.0.42",
            "27",
            "0123456789abcdef0123456789abcdef",
            "Linux",
            "Linux PC",
        );
        assert_eq!(parameters.password, "{MD5}52c2d750075c768ad3d5811af10df80e");
        assert_eq!(
            parameters.info,
            "{SRBX1}+Yd2acmMz0EhMm18ImhS3yh2JsNR2JDBsm0PWnxI7gircMegasQYAsV65OnYxZObRMnLOSEP0MHCvqfaSezJDE2B1cAVXqTEQnfbS8InAAPMyZkj490aDNJyekb3frQ2cBggjS=="
        );
        assert_eq!(
            parameters.chksum,
            "5aeb46c1c6ef126fe3acd4452d4131916d48c4d8"
        );
    }
}
