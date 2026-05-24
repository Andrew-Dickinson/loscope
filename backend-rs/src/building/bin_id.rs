use std::fmt;
use arrayvec::ArrayString;
use rocket::serde::{Deserialize, Deserializer, Serialize, Serializer};
use rocket::serde::de::{Error, Unexpected, Visitor};
use crate::types::errors::BINParseError;
use crate::types::tiles::TileId;

const BIN_LENGTH_CHARS: usize = 7;
const PERMITTED_BIN_FIRST_CHAR: &[u8] = &[1, 2, 3, 4, 5];

#[derive(Debug, Clone, Copy)]
pub struct BINId(ArrayString<BIN_LENGTH_CHARS>);

impl Serialize for BINId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer
    {
        self.as_str().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for BINId {
    fn deserialize<D>(des: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>
    {
        pub struct BINIdVisitor;
        impl<'de> Visitor<'de> for BINIdVisitor {
            type Value = BINId;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "a 7-character digital bin id with a valid first char")
            }
            fn visit_str<E>(self, input_str: &str) -> Result<Self::Value, E>
            where
                E: Error,
            {
                BINId::parse(input_str).map_err(
                    |_| Error::invalid_type(Unexpected::Str(input_str), &self)
                )
            }
        }

        des.deserialize_str(BINIdVisitor)
    }
}

impl BINId {
    pub fn from_int(bin_id: i64) -> Result<BINId, BINParseError> {
        BINId::parse(&*bin_id.to_string())
    }

    pub fn parse(bin_id: &str) -> Result<BINId, BINParseError> {
        let chars: Vec<char> = bin_id.chars().collect();

        if chars.len() != BIN_LENGTH_CHARS {
            return Err(BINParseError(format!("Invalid BIN ID: {bin_id}. Expected {BIN_LENGTH_CHARS} chars")));
        }

        let Some(first_digit) = chars[0].to_digit(10) else {
            return Err(BINParseError(format!("Invalid BIN ID: {bin_id}. All characters must be digits")));
        };
        // Safety: first_digit < 10, so it will always fit into a u8
        if !PERMITTED_BIN_FIRST_CHAR.contains(&(first_digit.try_into().unwrap())) {
            return Err(BINParseError(format!("Invalid BIN ID: {bin_id}. First character must be one of {PERMITTED_BIN_FIRST_CHAR:?}")));
        };

        for c in chars {
            if !c.is_digit(10) {
                return Err(BINParseError(format!("Invalid BIN ID: {bin_id}. All characters must be digits")));
            }
        }

        // Safety: The below .unwrap() is safe because we validate chars.len() == BIN_LENGTH_CHARS
        // above, and BINId.0 is of type ArrayString<BIN_LENGTH_CHARS>
        Ok(BINId(ArrayString::from(bin_id).unwrap()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}


#[cfg(test)]
mod tests {
    // --- BINId::parse ---

    use crate::building::bin_id::BINId;

    #[test]
    fn bin_id_parse_valid() {
        let id = BINId::parse("1234567").unwrap();
        assert_eq!(id.as_str(), "1234567");
    }

    #[test]
    fn bin_id_parse_all_valid_first_chars() {
        for c in [b'1', b'2', b'3', b'4', b'5'] {
            let s = format!("{}000000", c as char);
            assert!(BINId::parse(&s).is_ok(), "first char '{}' should be valid", c as char);
        }
    }

    #[test]
    fn bin_id_parse_too_short() {
        assert!(BINId::parse("123456").is_err());
    }

    #[test]
    fn bin_id_parse_too_long() {
        assert!(BINId::parse("12345678").is_err());
    }

    #[test]
    fn bin_id_parse_empty() {
        assert!(BINId::parse("").is_err());
    }

    #[test]
    fn bin_id_parse_first_char_zero() {
        assert!(BINId::parse("0123456").is_err());
    }

    #[test]
    fn bin_id_parse_first_char_six_to_nine() {
        for c in [b'6', b'7', b'8', b'9'] {
            let s = format!("{}000000", c as char);
            assert!(BINId::parse(&s).is_err(), "first char '{}' should be invalid", c as char);
        }
    }

    #[test]
    fn bin_id_parse_non_digit() {
        assert!(BINId::parse("1a34567").is_err());
    }
}