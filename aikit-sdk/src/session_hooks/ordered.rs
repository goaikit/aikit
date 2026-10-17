//! A JSON value that keeps object keys in the order they were read, so a
//! settings file is rewritten with its keys where the user put them.

use std::fmt;

use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::ser::{Serialize, SerializeMap, Serializer};

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Json {
    Null,
    Bool(bool),
    Number(serde_json::Number),
    String(String),
    Array(Vec<Json>),
    Object(Obj),
}

/// An object's members, in order.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct Obj(Vec<(String, Json)>);

impl Obj {
    pub(super) fn get(&self, key: &str) -> Option<&Json> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub(super) fn get_mut(&mut self, key: &str) -> Option<&mut Json> {
        self.0.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// Replaces the value of `key` in place, or appends it.
    pub(super) fn insert(&mut self, key: &str, value: Json) {
        match self.get_mut(key) {
            Some(v) => *v = value,
            None => self.0.push((key.to_string(), value)),
        }
    }

    /// Puts `key` first, before every other member; `key` must be absent.
    pub(super) fn insert_first(&mut self, key: &str, value: Json) {
        self.0.insert(0, (key.to_string(), value));
    }

    pub(super) fn get_or_insert_with(&mut self, key: &str, f: impl FnOnce() -> Json) -> &mut Json {
        let i = match self.0.iter().position(|(k, _)| k == key) {
            Some(i) => i,
            None => {
                self.0.push((key.to_string(), f()));
                self.0.len() - 1
            }
        };
        &mut self.0[i].1
    }

    /// Removes `key`, keeping the other members in order.
    pub(super) fn shift_remove(&mut self, key: &str) {
        self.0.retain(|(k, _)| k != key);
    }

    pub(super) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Json {
    pub(super) fn get(&self, key: &str) -> Option<&Json> {
        self.as_object().and_then(|o| o.get(key))
    }

    pub(super) fn get_mut(&mut self, key: &str) -> Option<&mut Json> {
        self.as_object_mut().and_then(|o| o.get_mut(key))
    }

    pub(super) fn as_str(&self) -> Option<&str> {
        match self {
            Json::String(s) => Some(s),
            _ => None,
        }
    }

    pub(super) fn as_array(&self) -> Option<&Vec<Json>> {
        match self {
            Json::Array(a) => Some(a),
            _ => None,
        }
    }

    pub(super) fn as_array_mut(&mut self) -> Option<&mut Vec<Json>> {
        match self {
            Json::Array(a) => Some(a),
            _ => None,
        }
    }

    pub(super) fn as_object(&self) -> Option<&Obj> {
        match self {
            Json::Object(o) => Some(o),
            _ => None,
        }
    }

    pub(super) fn as_object_mut(&mut self) -> Option<&mut Obj> {
        match self {
            Json::Object(o) => Some(o),
            _ => None,
        }
    }

    pub(super) fn is_array(&self) -> bool {
        matches!(self, Json::Array(_))
    }

    pub(super) fn is_object(&self) -> bool {
        matches!(self, Json::Object(_))
    }
}

impl Serialize for Json {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Json::Null => s.serialize_unit(),
            Json::Bool(b) => s.serialize_bool(*b),
            Json::Number(n) => n.serialize(s),
            Json::String(v) => s.serialize_str(v),
            Json::Array(a) => a.serialize(s),
            Json::Object(o) => {
                let mut map = s.serialize_map(Some(o.0.len()))?;
                for (k, v) in &o.0 {
                    map.serialize_entry(k, v)?;
                }
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for Json {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Json, D::Error> {
        d.deserialize_any(JsonVisitor)
    }
}

struct JsonVisitor;

impl<'de> Visitor<'de> for JsonVisitor {
    type Value = Json;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("any JSON value")
    }

    fn visit_unit<E>(self) -> Result<Json, E> {
        Ok(Json::Null)
    }

    fn visit_bool<E>(self, v: bool) -> Result<Json, E> {
        Ok(Json::Bool(v))
    }

    fn visit_i64<E>(self, v: i64) -> Result<Json, E> {
        Ok(Json::Number(v.into()))
    }

    fn visit_u64<E>(self, v: u64) -> Result<Json, E> {
        Ok(Json::Number(v.into()))
    }

    fn visit_f64<E>(self, v: f64) -> Result<Json, E> {
        Ok(serde_json::Number::from_f64(v).map_or(Json::Null, Json::Number))
    }

    fn visit_str<E>(self, v: &str) -> Result<Json, E> {
        Ok(Json::String(v.to_string()))
    }

    fn visit_string<E>(self, v: String) -> Result<Json, E> {
        Ok(Json::String(v))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Json, A::Error> {
        let mut out = Vec::new();
        while let Some(v) = seq.next_element()? {
            out.push(v);
        }
        Ok(Json::Array(out))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Json, A::Error> {
        let mut out = Vec::new();
        while let Some((k, v)) = map.next_entry::<String, Json>()? {
            out.push((k, v));
        }
        Ok(Json::Object(Obj(out)))
    }
}
