use std::fmt;
use rocket::serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use rocket::serde::de::{SeqAccess, Visitor};
use rocket::serde::json::serde_json;
use rocket::serde::ser::SerializeTuple;
use crate::types::coords::{NYSCoords2, NYSCoords3};

pub struct CoordsVisitor3;
impl<'de> Visitor<'de> for CoordsVisitor3 {
    type Value = (f64, f64, f64);
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "a tuple of three floats")
    }
    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where A: SeqAccess<'de> {
        let e: f64 = seq.next_element()?.ok_or_else(|| de::Error::invalid_length(0, &self))?;
        let n: f64 = seq.next_element()?.ok_or_else(|| de::Error::invalid_length(1, &self))?;
        let z: f64 = seq.next_element()?.ok_or_else(|| de::Error::invalid_length(2, &self))?;
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
    where A: SeqAccess<'de> {
        let e: f64 = seq.next_element()?.ok_or_else(|| de::Error::invalid_length(0, &self))?;
        let n: f64 = seq.next_element()?.ok_or_else(|| de::Error::invalid_length(1, &self))?;
        Ok((e, n))
    }
}


pub fn serialize_tuple2<S: Serializer>(tuple: (f64, f64), serializer: S) -> Result<S::Ok, S::Error> {
    let mut seq = serializer.serialize_tuple(3)?;
    seq.serialize_element(&tuple.0)?;
    seq.serialize_element(&tuple.1)?;
    seq.end()
}

pub fn serialize_tuple3<S: Serializer>(tuple: (f64, f64, f64), serializer: S) -> Result<S::Ok, S::Error> {
let mut seq = serializer.serialize_tuple(3)?;
    seq.serialize_element(&tuple.0)?;
    seq.serialize_element(&tuple.1)?;
    seq.serialize_element(&tuple.2)?;
    seq.end()
}
