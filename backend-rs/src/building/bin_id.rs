use crate::types::errors::BINParseError;
use arrayvec::ArrayString;
use rocket::serde::de::{Error, Unexpected, Visitor};
use rocket::serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

const BIN_LENGTH_CHARS: usize = 7;
const PERMITTED_BIN_FIRST_CHAR: &[u8] = &[1, 2, 3, 4, 5];

#[derive(Debug, Clone, Copy)]
pub struct BINId(ArrayString<BIN_LENGTH_CHARS>);

impl Serialize for BINId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.as_str().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for BINId {
    fn deserialize<D>(des: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
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
                BINId::parse(input_str)
                    .map_err(|_| Error::invalid_type(Unexpected::Str(input_str), &self))
            }
        }

        des.deserialize_str(BINIdVisitor)
    }
}

impl BINId {
    pub fn from_int(bin_id: i64) -> Result<BINId, BINParseError> {
        BINId::parse(&bin_id.to_string())
    }

    pub fn parse(bin_id: &str) -> Result<BINId, BINParseError> {
        let chars: Vec<char> = bin_id.chars().collect();

        if chars.len() != BIN_LENGTH_CHARS {
            return Err(BINParseError(format!(
                "Invalid BIN ID: {bin_id}. Expected {BIN_LENGTH_CHARS} chars"
            )));
        }

        let Some(first_digit) = chars[0].to_digit(10) else {
            return Err(BINParseError(format!(
                "Invalid BIN ID: {bin_id}. All characters must be digits"
            )));
        };
        // Safety: first_digit < 10, so it will always fit into a u8
        if !PERMITTED_BIN_FIRST_CHAR.contains(&(first_digit.try_into().unwrap())) {
            return Err(BINParseError(format!(
                "Invalid BIN ID: {bin_id}. First character must be one of {PERMITTED_BIN_FIRST_CHAR:?}"
            )));
        };

        for c in chars {
            if !c.is_ascii_digit() {
                return Err(BINParseError(format!(
                    "Invalid BIN ID: {bin_id}. All characters must be digits"
                )));
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
    use serde_json;

    #[test]
    fn bin_id_parse_valid() {
        let id = BINId::parse("1234567").unwrap();
        assert_eq!(id.as_str(), "1234567");
    }

    #[test]
    fn bin_id_parse_all_valid_first_chars() {
        for c in [b'1', b'2', b'3', b'4', b'5'] {
            let s = format!("{}000000", c as char);
            assert!(
                BINId::parse(&s).is_ok(),
                "first char '{}' should be valid",
                c as char
            );
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
            assert!(
                BINId::parse(&s).is_err(),
                "first char '{}' should be invalid",
                c as char
            );
        }
    }

    #[test]
    fn bin_id_parse_non_digit() {
        assert!(BINId::parse("1a34567").is_err());
    }

    // --- BINId::from_int ---

    #[test]
    fn bin_id_from_int_valid() {
        let id = BINId::from_int(1234567).unwrap();
        assert_eq!(id.as_str(), "1234567");
    }

    #[test]
    fn bin_id_from_int_all_valid_first_digits() {
        for d in [1i64, 2, 3, 4, 5] {
            assert!(BINId::from_int(d * 1_000_000).is_ok());
        }
    }

    #[test]
    fn bin_id_from_int_too_small() {
        assert!(BINId::from_int(123456).is_err());
    }

    #[test]
    fn bin_id_from_int_too_large() {
        assert!(BINId::from_int(12345678).is_err());
    }

    #[test]
    fn bin_id_from_int_invalid_first_digit() {
        assert!(BINId::from_int(6000000).is_err());
    }

    #[test]
    fn bin_id_from_int_zero_first_digit() {
        // 7-digit number starting with 0 cannot be represented as i64 with leading zero,
        // so from_int(0123456) == from_int(123456) which is too short — confirm it errors
        assert!(BINId::from_int(123_456).is_err());
    }

    // --- Serde ---

    #[test]
    fn bin_id_serialize() {
        let id = BINId::parse("3456789").unwrap();
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"3456789\"");
    }

    #[test]
    fn bin_id_deserialize_valid() {
        let id: BINId = serde_json::from_str("\"2345678\"").unwrap();
        assert_eq!(id.as_str(), "2345678");
    }

    #[test]
    fn bin_id_serde_roundtrip() {
        let original = BINId::parse("1234567").unwrap();
        let json = serde_json::to_string(&original).unwrap();
        let restored: BINId = serde_json::from_str(&json).unwrap();
        assert_eq!(original.as_str(), restored.as_str());
    }

    #[test]
    fn bin_id_deserialize_too_short() {
        assert!(serde_json::from_str::<BINId>("\"123456\"").is_err());
    }

    #[test]
    fn bin_id_deserialize_too_long() {
        assert!(serde_json::from_str::<BINId>("\"12345678\"").is_err());
    }

    #[test]
    fn bin_id_deserialize_invalid_first_char() {
        assert!(serde_json::from_str::<BINId>("\"0123456\"").is_err());
        assert!(serde_json::from_str::<BINId>("\"6123456\"").is_err());
    }

    #[test]
    fn bin_id_deserialize_non_digit() {
        assert!(serde_json::from_str::<BINId>("\"1a34567\"").is_err());
    }

    #[test]
    fn bin_id_deserialize_not_a_string() {
        assert!(serde_json::from_str::<BINId>("1234567").is_err());
    }
}
