use crate::types::coords::{NYSCoords2, NYSCoords3};
use rocket::serde::de::{SeqAccess, Visitor};
use rocket::serde::json::serde_json;
use rocket::serde::ser::SerializeTuple;
use rocket::serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use std::fmt;

pub struct CoordsVisitor3;
impl<'de> Visitor<'de> for CoordsVisitor3 {
    type Value = (f64, f64, f64);
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "a tuple of three floats")
    }
    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let e: f64 = seq
            .next_element()?
            .ok_or_else(|| de::Error::invalid_length(0, &self))?;
        let n: f64 = seq
            .next_element()?
            .ok_or_else(|| de::Error::invalid_length(1, &self))?;
        let z: f64 = seq
            .next_element()?
            .ok_or_else(|| de::Error::invalid_length(2, &self))?;
        Ok((e, n, z))
    }
}

pub struct CoordsVisitor2;
impl<'de> Visitor<'de> for CoordsVisitor2 {
    type Value = (f64, f64);
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "a tuple of two floats")
    }
    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let e: f64 = seq
            .next_element()?
            .ok_or_else(|| de::Error::invalid_length(0, &self))?;
        let n: f64 = seq
            .next_element()?
            .ok_or_else(|| de::Error::invalid_length(1, &self))?;
        Ok((e, n))
    }
}

pub fn serialize_tuple2<S: Serializer>(
    tuple: (f64, f64),
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let mut seq = serializer.serialize_tuple(2)?;
    seq.serialize_element(&tuple.0)?;
    seq.serialize_element(&tuple.1)?;
    seq.end()
}

pub fn serialize_tuple3<S: Serializer>(
    tuple: (f64, f64, f64),
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let mut seq = serializer.serialize_tuple(3)?;
    seq.serialize_element(&tuple.0)?;
    seq.serialize_element(&tuple.1)?;
    seq.serialize_element(&tuple.2)?;
    seq.end()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rocket::serde::json::serde_json;

    // Thin wrappers so we can call serde_json::{to_string, from_str} on the functions under test.
    #[derive(Serialize)]
    struct W2(#[serde(serialize_with = "serialize_tuple2_ref")] (f64, f64));
    fn serialize_tuple2_ref<S: Serializer>(v: &(f64, f64), s: S) -> Result<S::Ok, S::Error> {
        serialize_tuple2(*v, s)
    }

    #[derive(Serialize)]
    struct W3(#[serde(serialize_with = "serialize_tuple3_ref")] (f64, f64, f64));
    fn serialize_tuple3_ref<S: Serializer>(v: &(f64, f64, f64), s: S) -> Result<S::Ok, S::Error> {
        serialize_tuple3(*v, s)
    }

    fn deser2(json: &str) -> Result<(f64, f64), serde_json::Error> {
        let mut de = serde_json::Deserializer::from_str(json);
        de.deserialize_seq(CoordsVisitor2)
    }

    fn deser3(json: &str) -> Result<(f64, f64, f64), serde_json::Error> {
        let mut de = serde_json::Deserializer::from_str(json);
        de.deserialize_seq(CoordsVisitor3)
    }

    // --- serialize_tuple2 ---

    #[test]
    fn serialize_tuple2_produces_json_array() {
        assert_eq!(serde_json::to_string(&W2((1.0, 2.0))).unwrap(), "[1.0,2.0]");
    }

    #[test]
    fn serialize_tuple2_negative_values() {
        assert_eq!(
            serde_json::to_string(&W2((-3.5, 0.0))).unwrap(),
            "[-3.5,0.0]"
        );
    }

    // --- serialize_tuple3 ---

    #[test]
    fn serialize_tuple3_produces_json_array() {
        assert_eq!(
            serde_json::to_string(&W3((1.0, 2.0, 3.0))).unwrap(),
            "[1.0,2.0,3.0]"
        );
    }

    #[test]
    fn serialize_tuple3_negative_values() {
        assert_eq!(
            serde_json::to_string(&W3((-1.0, 0.0, 999.5))).unwrap(),
            "[-1.0,0.0,999.5]"
        );
    }

    // --- CoordsVisitor2 ---

    #[test]
    fn coords_visitor2_parses_two_element_array() {
        assert_eq!(deser2("[10.0, 20.0]").unwrap(), (10.0, 20.0));
    }

    #[test]
    fn coords_visitor2_roundtrip() {
        let original = (123.456, -78.9);
        let json = serde_json::to_string(&W2(original)).unwrap();
        assert_eq!(deser2(&json).unwrap(), original);
    }

    #[test]
    fn coords_visitor2_empty_array_errors() {
        assert!(deser2("[]").is_err());
    }

    #[test]
    fn coords_visitor2_one_element_errors() {
        assert!(deser2("[1.0]").is_err());
    }

    // --- CoordsVisitor3 ---

    #[test]
    fn coords_visitor3_parses_three_element_array() {
        assert_eq!(deser3("[1.0, 2.0, 3.0]").unwrap(), (1.0, 2.0, 3.0));
    }

    #[test]
    fn coords_visitor3_roundtrip() {
        let original = (100.0, 200.0, 300.0);
        let json = serde_json::to_string(&W3(original)).unwrap();
        assert_eq!(deser3(&json).unwrap(), original);
    }

    #[test]
    fn coords_visitor3_empty_array_errors() {
        assert!(deser3("[]").is_err());
    }

    #[test]
    fn coords_visitor3_two_elements_errors() {
        assert!(deser3("[1.0, 2.0]").is_err());
    }
}
