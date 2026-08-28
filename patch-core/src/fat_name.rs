use std::fmt;

use anyhow::{Result, ensure};
use serde::{Deserialize, Deserializer, Serialize};

const RAW_SFN_BYTES: usize = 11;
const MAX_RAW_LHA_NAME_BYTES: usize = 255;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(untagged)]
pub enum FatShortName {
    Ascii(String),
    Raw { raw_sfn_hex: String },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFatShortName {
    raw_sfn_hex: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum FatShortNameRepresentation {
    Ascii(String),
    Raw(RawFatShortName),
}

impl<'de> Deserialize<'de> for FatShortName {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(
            match FatShortNameRepresentation::deserialize(deserializer)? {
                FatShortNameRepresentation::Ascii(name) => Self::Ascii(name),
                FatShortNameRepresentation::Raw(raw) => Self::Raw {
                    raw_sfn_hex: raw.raw_sfn_hex,
                },
            },
        )
    }
}

impl FatShortName {
    pub fn ascii(name: impl Into<String>) -> Self {
        Self::Ascii(name.into())
    }

    pub fn validate(&self, label: &str) -> Result<()> {
        match self {
            Self::Ascii(name) => validate_ascii_dos_name(name, label),
            Self::Raw { raw_sfn_hex } => {
                let raw = decode_lower_hex_exact::<RAW_SFN_BYTES>(raw_sfn_hex, label)?;
                ensure!(
                    raw[0] != 0x00 && raw[0] != 0xe5,
                    "{label} raw SFN starts with a FAT12 free-entry marker"
                );
                ensure!(!raw.contains(&0x00), "{label} raw SFN contains a zero byte");
                ensure!(
                    raw[..8].iter().any(|byte| *byte != b' '),
                    "{label} raw SFN has an empty stem"
                );
                ensure!(
                    !(raw[0] == b'.' && matches!(raw[1], b' ' | b'.')),
                    "{label} raw SFN cannot be a dot directory entry"
                );
                Ok(())
            }
        }
    }

    pub fn raw_bytes(&self, label: &str) -> Result<[u8; RAW_SFN_BYTES]> {
        self.validate(label)?;
        match self {
            Self::Ascii(name) => ascii_dos_name_bytes(name),
            Self::Raw { raw_sfn_hex } => decode_lower_hex_exact(raw_sfn_hex, label),
        }
    }

    pub fn ascii_name(&self) -> Option<&str> {
        match self {
            Self::Ascii(name) => Some(name),
            Self::Raw { .. } => None,
        }
    }

    pub fn raw_hex(&self, label: &str) -> Result<String> {
        Ok(encode_lower_hex(&self.raw_bytes(label)?))
    }

    pub fn is_raw(&self) -> bool {
        matches!(self, Self::Raw { .. })
    }
}

impl fmt::Display for FatShortName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ascii(name) => formatter.write_str(name),
            Self::Raw { raw_sfn_hex } => write!(formatter, "raw-sfn:{raw_sfn_hex}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(untagged)]
pub enum LhaMemberName {
    Ascii(String),
    Raw { raw_name_hex: String },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLhaMemberName {
    raw_name_hex: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum LhaMemberNameRepresentation {
    Ascii(String),
    Raw(RawLhaMemberName),
}

impl<'de> Deserialize<'de> for LhaMemberName {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(
            match LhaMemberNameRepresentation::deserialize(deserializer)? {
                LhaMemberNameRepresentation::Ascii(name) => Self::Ascii(name),
                LhaMemberNameRepresentation::Raw(raw) => Self::Raw {
                    raw_name_hex: raw.raw_name_hex,
                },
            },
        )
    }
}

impl LhaMemberName {
    pub fn ascii(name: impl Into<String>) -> Self {
        Self::Ascii(name.into())
    }

    pub fn validate(&self, label: &str) -> Result<()> {
        match self {
            Self::Ascii(name) => validate_ascii_dos_name(name, label),
            Self::Raw { raw_name_hex } => {
                ensure!(!raw_name_hex.is_empty(), "{label} raw name cannot be empty");
                ensure!(
                    raw_name_hex.len() <= MAX_RAW_LHA_NAME_BYTES * 2,
                    "{label} raw name is longer than {MAX_RAW_LHA_NAME_BYTES} bytes"
                );
                validate_lower_hex(raw_name_hex, label)
            }
        }
    }

    pub fn matches(&self, parsed_ascii_name: &str, raw_name: &[u8]) -> Result<bool> {
        self.validate("LHA member")?;
        match self {
            Self::Ascii(name) => Ok(name == parsed_ascii_name),
            Self::Raw { raw_name_hex } => {
                Ok(decode_lower_hex(raw_name_hex, "LHA member")? == raw_name)
            }
        }
    }
}

impl fmt::Display for LhaMemberName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ascii(name) => formatter.write_str(name),
            Self::Raw { raw_name_hex } => write!(formatter, "raw-lha-name:{raw_name_hex}"),
        }
    }
}

pub(crate) fn validate_ascii_dos_name(name: &str, label: &str) -> Result<()> {
    ensure!(
        !name.is_empty() && name == name.to_ascii_uppercase(),
        "{label} must be an uppercase DOS 8.3 name: {name:?}"
    );
    ensure!(
        name.bytes()
            .all(|byte| byte.is_ascii_alphanumeric()
                || matches!(byte, b'_' | b'$' | b'~' | b'-' | b'.')),
        "{label} contains unsupported characters: {name:?}"
    );
    let mut parts = name.split('.');
    let stem = parts.next().unwrap_or_default();
    let extension = parts.next();
    ensure!(
        parts.next().is_none(),
        "{label} is not a DOS 8.3 name: {name:?}"
    );
    ensure!(
        (1..=8).contains(&stem.len()),
        "{label} stem is not 1..=8 bytes: {name:?}"
    );
    if let Some(extension) = extension {
        ensure!(
            (1..=3).contains(&extension.len()),
            "{label} extension is not 1..=3 bytes: {name:?}"
        );
    }
    Ok(())
}

fn ascii_dos_name_bytes(name: &str) -> Result<[u8; RAW_SFN_BYTES]> {
    validate_ascii_dos_name(name, "DOS name")?;
    let (stem, extension) = name
        .split_once('.')
        .map_or((name, ""), |(stem, extension)| (stem, extension));
    let mut raw = [b' '; RAW_SFN_BYTES];
    raw[..stem.len()].copy_from_slice(stem.as_bytes());
    raw[8..8 + extension.len()].copy_from_slice(extension.as_bytes());
    Ok(raw)
}

fn decode_lower_hex_exact<const N: usize>(value: &str, label: &str) -> Result<[u8; N]> {
    ensure!(
        value.len() == N * 2,
        "{label} must encode exactly {N} bytes, got {}",
        value.len() / 2
    );
    let bytes = decode_lower_hex(value, label)?;
    bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("{label} has an invalid decoded length"))
}

fn decode_lower_hex(value: &str, label: &str) -> Result<Vec<u8>> {
    validate_lower_hex(value, label)?;
    value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let high = hex_nibble(pair[0]);
            let low = hex_nibble(pair[1]);
            Ok((high << 4) | low)
        })
        .collect()
}

fn validate_lower_hex(value: &str, label: &str) -> Result<()> {
    ensure!(
        value.len().is_multiple_of(2),
        "{label} raw hex must contain whole bytes"
    );
    ensure!(
        value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "{label} raw hex must use lowercase hexadecimal"
    );
    Ok(())
}

fn hex_nibble(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        _ => unreachable!("hex input was validated"),
    }
}

fn encode_lower_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
#[path = "fat_name_tests.rs"]
mod fat_name_tests;
