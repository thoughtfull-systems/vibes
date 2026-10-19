// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use std::fmt;
use std::str::FromStr;

use uuid::Uuid;

const ALPHABET: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";
const LENGTH: usize = 26;

/// The permanent identifier of a note, as the links spec defines it.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct NoteId(u128);

impl NoteId {
    pub fn generate() -> Self {
        Self(Uuid::now_v7().as_u128())
    }

    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid.as_u128())
    }

    pub fn uuid(self) -> Uuid {
        Uuid::from_u128(self.0)
    }
}

impl fmt::Display for NoteId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut digits = [0u8; LENGTH];
        for (i, digit) in digits.iter_mut().rev().enumerate() {
            *digit = ALPHABET[((self.0 >> (5 * i)) & 0x1f) as usize];
        }
        // The alphabet is ASCII.
        f.write_str(std::str::from_utf8(&digits).unwrap())
    }
}

#[derive(Debug, Eq, PartialEq, thiserror::Error)]
#[error("not a note identifier: {0:?}")]
pub struct ParseNoteIdError(pub String);

impl FromStr for NoteId {
    type Err = ParseNoteIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let error = || ParseNoteIdError(s.to_owned());
        if s.len() != LENGTH {
            return Err(error());
        }
        let mut value: u128 = 0;
        for (i, byte) in s.bytes().enumerate() {
            let lower = byte.to_ascii_lowercase();
            let digit = ALPHABET.iter().position(|&a| a == lower).ok_or_else(error)?;
            if i == 0 && digit > 7 {
                return Err(error());
            }
            value = (value << 5) | digit as u128;
        }
        Ok(Self(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        for _ in 0..100 {
            let id = NoteId::generate();
            assert_eq!(id.to_string().parse::<NoteId>(), Ok(id));
        }
    }

    #[test]
    fn encodes_extremes() {
        assert_eq!(NoteId(0).to_string(), "0".repeat(26));
        assert_eq!(NoteId(u128::MAX).to_string(), format!("7{}", "z".repeat(25)));
    }

    #[test]
    fn matches_spec_example() {
        let uuid = Uuid::parse_str("01920000-0000-7000-8000-000000000000").unwrap();
        assert_eq!(NoteId::from_uuid(uuid).to_string(), "01j8000000e008000000000000");
    }

    #[test]
    fn accepts_uppercase() {
        let id = NoteId::generate();
        assert_eq!(id.to_string().to_uppercase().parse::<NoteId>(), Ok(id));
    }

    #[test]
    fn rejects_invalid_text() {
        for text in [
            "",
            "0000000000000000000000000",
            "000000000000000000000000000",
            "80000000000000000000000000",
            "0000000000000000000000000i",
            "0000000000000000000000000u",
            "000000000000-0000000000000",
        ] {
            assert!(text.parse::<NoteId>().is_err(), "{text:?}");
        }
    }
}
