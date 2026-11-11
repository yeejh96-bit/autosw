//! Values used while laying out a document, before the JSON reporting boundary.
//! IEEE-754 numbers and undefined properties must survive all intermediate work.
use std::{
    borrow::Borrow,
    collections::HashMap,
    hash::Hash,
    ops::{Index, IndexMut},
    slice,
};

#[derive(Clone, Debug, PartialEq)]
pub enum NativeValue {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<NativeValue>),
    Object(Map<String, NativeValue>),
}
pub type Value = NativeValue;

/// Ordered property storage. Replacing a key retains its first insertion slot.
#[derive(Clone, Debug)]
pub struct Map<K = String, V = NativeValue> {
    entries: Vec<(K, V)>,
    positions: HashMap<K, usize>,
}
impl<K: PartialEq, V: PartialEq> PartialEq for Map<K, V> {
    fn eq(&self, other: &Self) -> bool {
        self.entries == other.entries
    }
}
impl<K, V> Default for Map<K, V> {
    fn default() -> Self {
        Self::new()
    }
}
impl<K, V> Map<K, V> {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            positions: HashMap::new(),
        }
    }
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: Vec::with_capacity(capacity),
            positions: HashMap::with_capacity(capacity),
        }
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn clear(&mut self) {
        self.entries.clear();
        self.positions.clear();
    }
    pub fn iter(&self) -> Iter<'_, K, V> {
        Iter {
            inner: self.entries.iter(),
        }
    }
    pub fn iter_mut(&mut self) -> IterMut<'_, K, V> {
        IterMut {
            inner: self.entries.iter_mut(),
        }
    }
    pub fn keys(&self) -> impl DoubleEndedIterator<Item = &K> + ExactSizeIterator {
        self.entries.iter().map(|(key, _)| key)
    }
    pub fn values(&self) -> impl DoubleEndedIterator<Item = &V> + ExactSizeIterator {
        self.entries.iter().map(|(_, value)| value)
    }
    pub fn values_mut(&mut self) -> impl DoubleEndedIterator<Item = &mut V> + ExactSizeIterator {
        self.entries.iter_mut().map(|(_, value)| value)
    }
    pub fn get<Q: ?Sized + Eq + Hash>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q> + Eq + Hash,
    {
        self.positions.get(key).map(|&index| &self.entries[index].1)
    }
    pub fn get_mut<Q: ?Sized + Eq + Hash>(&mut self, key: &Q) -> Option<&mut V>
    where
        K: Borrow<Q> + Eq + Hash,
    {
        let &index = self.positions.get(key)?;
        Some(&mut self.entries[index].1)
    }
    pub fn contains_key<Q: ?Sized + Eq + Hash>(&self, key: &Q) -> bool
    where
        K: Borrow<Q> + Eq + Hash,
    {
        self.get(key).is_some()
    }
    pub fn shift_remove<Q: ?Sized + Eq + Hash>(&mut self, key: &Q) -> Option<V>
    where
        K: Borrow<Q> + Eq + Hash,
    {
        let index = self.positions.remove(key)?;
        let value = self.entries.remove(index).1;
        for (index, (key, _)) in self.entries.iter().enumerate().skip(index) {
            *self.positions.get_mut::<K>(key).unwrap() = index;
        }
        Some(value)
    }
    pub fn remove<Q: ?Sized + Eq + Hash>(&mut self, key: &Q) -> Option<V>
    where
        K: Borrow<Q> + Eq + Hash,
    {
        self.shift_remove(key)
    }
}
impl<K: Eq + Hash + Clone, V> Map<K, V> {
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        if let Some(&index) = self.positions.get(&key) {
            Some(std::mem::replace(&mut self.entries[index].1, value))
        } else {
            self.positions.insert(key.clone(), self.entries.len());
            self.entries.push((key, value));
            None
        }
    }
}
impl<K: Eq + Hash + Clone, V> Extend<(K, V)> for Map<K, V> {
    fn extend<T: IntoIterator<Item = (K, V)>>(&mut self, items: T) {
        for (key, value) in items {
            self.insert(key, value);
        }
    }
}
impl<K: Eq + Hash + Clone, V> FromIterator<(K, V)> for Map<K, V> {
    fn from_iter<T: IntoIterator<Item = (K, V)>>(items: T) -> Self {
        let mut result = Self::new();
        result.extend(items);
        result
    }
}
pub struct Iter<'a, K, V> {
    inner: slice::Iter<'a, (K, V)>,
}
impl<'a, K, V> Iterator for Iter<'a, K, V> {
    type Item = (&'a K, &'a V);
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|(k, v)| (k, v))
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}
impl<K, V> DoubleEndedIterator for Iter<'_, K, V> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.inner.next_back().map(|(k, v)| (k, v))
    }
}
impl<K, V> ExactSizeIterator for Iter<'_, K, V> {}
impl<K, V> std::iter::FusedIterator for Iter<'_, K, V> {}
pub struct IterMut<'a, K, V> {
    inner: slice::IterMut<'a, (K, V)>,
}
impl<'a, K, V> Iterator for IterMut<'a, K, V> {
    type Item = (&'a K, &'a mut V);
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|(key, value)| (&*key, value))
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}
impl<K, V> DoubleEndedIterator for IterMut<'_, K, V> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.inner.next_back().map(|(key, value)| (&*key, value))
    }
}
impl<K, V> ExactSizeIterator for IterMut<'_, K, V> {}
impl<K, V> std::iter::FusedIterator for IterMut<'_, K, V> {}
impl<K, V> IntoIterator for Map<K, V> {
    type Item = (K, V);
    type IntoIter = std::vec::IntoIter<(K, V)>;
    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_iter()
    }
}
impl<'a, K, V> IntoIterator for &'a Map<K, V> {
    type Item = (&'a K, &'a V);
    type IntoIter = Iter<'a, K, V>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
impl<'a, K, V> IntoIterator for &'a mut Map<K, V> {
    type Item = (&'a K, &'a mut V);
    type IntoIter = IterMut<'a, K, V>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}
impl<K, V, Q: ?Sized + Eq + Hash> Index<&Q> for Map<K, V>
where
    K: Borrow<Q> + Eq + Hash,
{
    type Output = V;
    fn index(&self, key: &Q) -> &V {
        self.get(key).expect("native object key not present")
    }
}
impl<K, V, Q: ?Sized + Eq + Hash> IndexMut<&Q> for Map<K, V>
where
    K: Borrow<Q> + Eq + Hash,
{
    fn index_mut(&mut self, key: &Q) -> &mut V {
        self.get_mut(key).expect("native object key not present")
    }
}

impl Default for NativeValue {
    fn default() -> Self {
        Self::Null
    }
}
impl NativeValue {
    pub fn as_str(&self) -> Option<&str> {
        if let Self::String(value) = self {
            Some(value)
        } else {
            None
        }
    }
    pub fn as_f64(&self) -> Option<f64> {
        if let Self::Number(value) = self {
            Some(*value)
        } else {
            None
        }
    }
    pub fn as_i64(&self) -> Option<i64> {
        self.as_f64()
            .filter(|n| {
                n.fract() == 0.0 && *n >= -9223372036854775808.0 && *n < 9223372036854775808.0
            })
            .map(|n| n as i64)
    }
    pub fn as_u64(&self) -> Option<u64> {
        self.as_f64()
            .filter(|n| n.fract() == 0.0 && *n >= 0.0 && *n < 18446744073709551616.0)
            .map(|n| n as u64)
    }
    pub fn as_bool(&self) -> Option<bool> {
        if let Self::Bool(value) = self {
            Some(*value)
        } else {
            None
        }
    }
    pub fn as_array(&self) -> Option<&Vec<Self>> {
        if let Self::Array(value) = self {
            Some(value)
        } else {
            None
        }
    }
    pub fn as_array_mut(&mut self) -> Option<&mut Vec<Self>> {
        if let Self::Array(value) = self {
            Some(value)
        } else {
            None
        }
    }
    pub fn as_object(&self) -> Option<&Map<String, Self>> {
        if let Self::Object(value) = self {
            Some(value)
        } else {
            None
        }
    }
    pub fn as_object_mut(&mut self) -> Option<&mut Map<String, Self>> {
        if let Self::Object(value) = self {
            Some(value)
        } else {
            None
        }
    }
    pub fn is_undefined(&self) -> bool {
        matches!(self, Self::Undefined)
    }
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }
    pub fn is_boolean(&self) -> bool {
        matches!(self, Self::Bool(_))
    }
    pub fn is_number(&self) -> bool {
        matches!(self, Self::Number(_))
    }
    pub fn is_string(&self) -> bool {
        matches!(self, Self::String(_))
    }
    pub fn is_array(&self) -> bool {
        matches!(self, Self::Array(_))
    }
    pub fn is_object(&self) -> bool {
        matches!(self, Self::Object(_))
    }
    pub fn get<I: ValueIndex>(&self, index: I) -> Option<&Self> {
        index.get(self)
    }
    pub fn get_mut<I: ValueIndex>(&mut self, index: I) -> Option<&mut Self> {
        index.get_mut(self)
    }
    pub fn from_json(value: &serde_json::Value) -> Self {
        match value {
            serde_json::Value::Null => Self::Null,
            serde_json::Value::Bool(value) => Self::Bool(*value),
            serde_json::Value::Number(value) => {
                Self::Number(value.as_f64().expect("JSON number representable as f64"))
            }
            serde_json::Value::String(value) => Self::String(value.clone()),
            serde_json::Value::Array(values) => {
                Self::Array(values.iter().map(Self::from_json).collect())
            }
            serde_json::Value::Object(values) => Self::Object(
                values
                    .iter()
                    .map(|(key, value)| (key.clone(), Self::from_json(value)))
                    .collect(),
            ),
        }
    }
    /// JSON.stringify-like reporting boundary. Undefined object members are
    /// absent; undefined array entries and non-finite numbers become null.
    /// Numeric-key ordering is left to the existing JSON.stringify writer.
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            Self::Undefined | Self::Null => serde_json::Value::Null,
            Self::Bool(value) => (*value).into(),
            Self::Number(value) if *value == 0.0 => 0.into(),
            Self::Number(value) => serde_json::Number::from_f64(*value)
                .map_or(serde_json::Value::Null, serde_json::Value::Number),
            Self::String(value) => value.clone().into(),
            Self::Array(values) => values.iter().map(Self::to_json).collect(),
            Self::Object(values) => serde_json::Value::Object(
                values
                    .iter()
                    .filter(|(_, value)| !value.is_undefined())
                    .map(|(key, value)| (key.clone(), value.to_json()))
                    .collect(),
            ),
        }
    }
}
pub fn from_json(value: &serde_json::Value) -> NativeValue {
    NativeValue::from_json(value)
}
pub fn to_json(value: &NativeValue) -> serde_json::Value {
    value.to_json()
}

/// JavaScript's default String conversion, matching input::js_string's plain
/// data contract. Objects do not execute user-supplied toString/valueOf code.
pub fn string(value: &NativeValue) -> String {
    match value {
        NativeValue::Undefined => "undefined".into(),
        NativeValue::Null => "null".into(),
        NativeValue::Bool(value) => value.to_string(),
        NativeValue::Number(value) if value.is_nan() => "NaN".into(),
        NativeValue::Number(value) if value.is_infinite() => if value.is_sign_negative() {
            "-Infinity"
        } else {
            "Infinity"
        }
        .into(),
        NativeValue::Number(value) if *value == 0.0 => "0".into(),
        NativeValue::Number(value) => ryu_js::Buffer::new().format(*value).to_owned(),
        NativeValue::String(value) => value.clone(),
        NativeValue::Array(values) => values
            .iter()
            .map(|value| {
                if matches!(value, NativeValue::Undefined | NativeValue::Null) {
                    String::new()
                } else {
                    string(value)
                }
            })
            .collect::<Vec<_>>()
            .join(","),
        NativeValue::Object(_) => "[object Object]".into(),
    }
}

/// JavaScript Number conversion with native numeric values kept intact. The
/// shared string parser handles whitespace/radices; only its string argument
/// crosses the serde boundary, never NaN, infinities or negative zero.
pub fn number(value: &NativeValue) -> f64 {
    match value {
        NativeValue::Number(value) => *value,
        NativeValue::Undefined => f64::NAN,
        NativeValue::Null => 0.0,
        NativeValue::Bool(value) => {
            if *value {
                1.0
            } else {
                0.0
            }
        }
        value => super::layout::number(Some(&serde_json::Value::String(string(value)))),
    }
}

/// Native indexing supports object names and existing array positions.
pub trait ValueIndex {
    fn get(self, value: &NativeValue) -> Option<&NativeValue>;
    fn get_mut(self, value: &mut NativeValue) -> Option<&mut NativeValue>;
    fn index_mut(self, value: &mut NativeValue) -> &mut NativeValue;
}
impl ValueIndex for &str {
    fn get(self, value: &NativeValue) -> Option<&NativeValue> {
        value.as_object()?.get(self)
    }
    fn get_mut(self, value: &mut NativeValue) -> Option<&mut NativeValue> {
        value.as_object_mut()?.get_mut(self)
    }
    fn index_mut(self, value: &mut NativeValue) -> &mut NativeValue {
        if matches!(value, NativeValue::Null | NativeValue::Undefined) {
            *value = NativeValue::Object(Map::new());
        }
        let object = value
            .as_object_mut()
            .expect("native value is not an object");
        if !object.contains_key(self) {
            object.insert(self.into(), NativeValue::Null);
        }
        object.get_mut(self).unwrap()
    }
}
impl ValueIndex for &String {
    fn get(self, value: &NativeValue) -> Option<&NativeValue> {
        ValueIndex::get(self.as_str(), value)
    }
    fn get_mut(self, value: &mut NativeValue) -> Option<&mut NativeValue> {
        ValueIndex::get_mut(self.as_str(), value)
    }
    fn index_mut(self, value: &mut NativeValue) -> &mut NativeValue {
        ValueIndex::index_mut(self.as_str(), value)
    }
}
impl ValueIndex for String {
    fn get(self, value: &NativeValue) -> Option<&NativeValue> {
        ValueIndex::get(self.as_str(), value)
    }
    fn get_mut(self, value: &mut NativeValue) -> Option<&mut NativeValue> {
        ValueIndex::get_mut(self.as_str(), value)
    }
    fn index_mut(self, value: &mut NativeValue) -> &mut NativeValue {
        ValueIndex::index_mut(self.as_str(), value)
    }
}
impl ValueIndex for usize {
    fn get(self, value: &NativeValue) -> Option<&NativeValue> {
        value.as_array()?.get(self)
    }
    fn get_mut(self, value: &mut NativeValue) -> Option<&mut NativeValue> {
        value.as_array_mut()?.get_mut(self)
    }
    fn index_mut(self, value: &mut NativeValue) -> &mut NativeValue {
        value
            .as_array_mut()
            .expect("native value is not an array")
            .get_mut(self)
            .expect("native array index out of bounds")
    }
}
impl<I: ValueIndex> Index<I> for NativeValue {
    type Output = Self;
    fn index(&self, index: I) -> &Self {
        index.get(self).unwrap_or(&Self::Undefined)
    }
}
impl<I: ValueIndex> IndexMut<I> for NativeValue {
    fn index_mut(&mut self, index: I) -> &mut Self {
        index.index_mut(self)
    }
}
impl From<&str> for NativeValue {
    fn from(value: &str) -> Self {
        Self::String(value.into())
    }
}
impl From<String> for NativeValue {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}
impl From<bool> for NativeValue {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}
macro_rules! numbers {
    ($($kind:ty),* $(,)?) => {$(
        impl From<$kind> for NativeValue {
            fn from(value: $kind) -> Self { Self::Number(value as f64) }
        }
        impl PartialEq<$kind> for NativeValue {
            fn eq(&self, other: &$kind) -> bool { self.as_f64() == Some(*other as f64) }
        }
        impl PartialEq<NativeValue> for $kind {
            fn eq(&self, other: &NativeValue) -> bool { other == self }
        }
    )*};
}
numbers!(f32, i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);
impl From<f64> for NativeValue {
    fn from(value: f64) -> Self {
        Self::Number(value)
    }
}
impl PartialEq<f64> for NativeValue {
    fn eq(&self, other: &f64) -> bool {
        self.as_f64() == Some(*other)
    }
}
impl PartialEq<NativeValue> for f64 {
    fn eq(&self, other: &NativeValue) -> bool {
        other == self
    }
}
impl<T: Into<NativeValue>> From<Vec<T>> for NativeValue {
    fn from(values: Vec<T>) -> Self {
        Self::Array(values.into_iter().map(Into::into).collect())
    }
}
impl<T: Clone + Into<NativeValue>> From<&[T]> for NativeValue {
    fn from(values: &[T]) -> Self {
        Self::Array(values.iter().cloned().map(Into::into).collect())
    }
}
impl<T: Into<NativeValue>, const N: usize> From<[T; N]> for NativeValue {
    fn from(values: [T; N]) -> Self {
        Self::Array(values.into_iter().map(Into::into).collect())
    }
}
impl<T: Into<NativeValue>> From<Option<T>> for NativeValue {
    fn from(value: Option<T>) -> Self {
        value.map_or(Self::Null, Into::into)
    }
}
impl From<Map<String, NativeValue>> for NativeValue {
    fn from(value: Map<String, NativeValue>) -> Self {
        Self::Object(value)
    }
}
impl<T: Clone + Into<NativeValue>> From<&T> for NativeValue {
    fn from(value: &T) -> Self {
        value.clone().into()
    }
}
impl<T: Into<NativeValue>> FromIterator<T> for NativeValue {
    fn from_iter<I: IntoIterator<Item = T>>(values: I) -> Self {
        Self::Array(values.into_iter().map(Into::into).collect())
    }
}
impl PartialEq<str> for NativeValue {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == Some(other)
    }
}
impl PartialEq<&str> for NativeValue {
    fn eq(&self, other: &&str) -> bool {
        self == *other
    }
}
impl PartialEq<String> for NativeValue {
    fn eq(&self, other: &String) -> bool {
        self == other.as_str()
    }
}
impl PartialEq<bool> for NativeValue {
    fn eq(&self, other: &bool) -> bool {
        self.as_bool() == Some(*other)
    }
}

/// Native expression construction. Expressions are borrowed and cloned, as
/// serde_json::json! does, but numbers never pass through serde_json::Number.
#[macro_export]
macro_rules! native_json {
    (@array $array:ident; ) => {};
    (@array $array:ident; ,) => {};
    (@array $array:ident; null, $($rest:tt)*) => {
        $array.push($crate::hwpx_layout::value::NativeValue::Null);
        $crate::native_json!(@array $array; $($rest)*);
    };
    (@array $array:ident; [$($inner:tt)*], $($rest:tt)*) => {
        $array.push($crate::native_json!([$($inner)*]));
        $crate::native_json!(@array $array; $($rest)*);
    };
    (@array $array:ident; {$($inner:tt)*}, $($rest:tt)*) => {
        $array.push($crate::native_json!({$($inner)*}));
        $crate::native_json!(@array $array; $($rest)*);
    };
    (@array $array:ident; $value:expr, $($rest:tt)*) => {
        $array.push($crate::native_json!($value));
        $crate::native_json!(@array $array; $($rest)*);
    };
    (@object $object:ident; ) => {};
    (@object $object:ident; ,) => {};
    (@object $object:ident; $key:tt : null, $($rest:tt)*) => {
        $object.insert(($key).to_string(), $crate::hwpx_layout::value::NativeValue::Null);
        $crate::native_json!(@object $object; $($rest)*);
    };
    (@object $object:ident; $key:tt : [$($inner:tt)*], $($rest:tt)*) => {
        $object.insert(($key).to_string(), $crate::native_json!([$($inner)*]));
        $crate::native_json!(@object $object; $($rest)*);
    };
    (@object $object:ident; $key:tt : {$($inner:tt)*}, $($rest:tt)*) => {
        $object.insert(($key).to_string(), $crate::native_json!({$($inner)*}));
        $crate::native_json!(@object $object; $($rest)*);
    };
    (@object $object:ident; $key:tt : $value:expr, $($rest:tt)*) => {
        $object.insert(($key).to_string(), $crate::native_json!($value));
        $crate::native_json!(@object $object; $($rest)*);
    };
    (null) => { $crate::hwpx_layout::value::NativeValue::Null };
    ([]) => { $crate::hwpx_layout::value::NativeValue::Array(::std::vec::Vec::new()) };
    ({}) => { $crate::hwpx_layout::value::NativeValue::Object($crate::hwpx_layout::value::Map::new()) };
    ([$($inner:tt)*]) => {{
        // The token parser must distinguish null/nested JSON from Rust
        // expressions before each push; this is not a hand-written vec list.
        #[allow(clippy::vec_init_then_push)]
        {
            let mut array = ::std::vec::Vec::<$crate::hwpx_layout::value::NativeValue>::new();
            $crate::native_json!(@array array; $($inner)* ,);
            $crate::hwpx_layout::value::NativeValue::Array(array)
        }
    }};
    ({$($inner:tt)*}) => {{
        let mut object = $crate::hwpx_layout::value::Map::new();
        $crate::native_json!(@object object; $($inner)* ,);
        $crate::hwpx_layout::value::NativeValue::Object(object)
    }};
    ($value:expr) => { $crate::hwpx_layout::value::NativeValue::from(&$value) };
}
pub use crate::native_json;

#[cfg(test)]
mod tests {
    use super::{Map, NativeValue, Value, native_json as json, number, string};
    #[test]
    fn ieee_numbers_are_unchanged_until_explicit_json_boundary() {
        let value = json!({"nan":f64::NAN,"positive":f64::INFINITY,"negative":f64::NEG_INFINITY,"zero":-0.0,"nested":[f64::NAN,{"infinity":f64::INFINITY}]});
        assert!(value["nan"].as_f64().unwrap().is_nan());
        assert_eq!(value["positive"].as_f64(), Some(f64::INFINITY));
        assert_eq!(value["negative"].as_f64(), Some(f64::NEG_INFINITY));
        assert!(value["zero"].as_f64().unwrap().is_sign_negative());
        assert!(value["nested"][0].as_f64().unwrap().is_nan());
        assert!(
            value["nested"][1]["infinity"]
                .as_f64()
                .unwrap()
                .is_infinite()
        );
        assert_eq!(
            value.to_json(),
            serde_json::json!({"nan":null,"positive":null,"negative":null,"zero":0,"nested":[null,{"infinity":null}]})
        );
        assert!(value["nan"].as_f64().unwrap().is_nan());
    }
    #[test]
    fn undefined_is_distinct_from_null_and_omitted_only_at_object_boundary() {
        let mut value = json!({"null":null,"absent":NativeValue::Undefined,"items":[NativeValue::Undefined,null]});
        assert!(value["null"].is_null());
        assert!(value["absent"].is_undefined());
        assert!(value["missing"].is_undefined());
        assert!(value.get("missing").is_none());
        assert!(value.get("absent").is_some());
        assert_eq!(
            value.to_json(),
            serde_json::json!({"null":null,"items":[null,null]})
        );
        value["new"]["nested"] = 12.into();
        assert_eq!(value["new"]["nested"], 12);
        let slot = &mut value["empty"];
        assert!(slot.is_null());
    }
    #[test]
    fn ordered_maps_keep_replacement_slots_and_support_borrowed_mutation() {
        let mut object: Map<String, Value> = Map::new();
        object.insert("10".into(), 1.into());
        object.insert("2".into(), 2.into());
        object.insert("name".into(), 3.into());
        assert_eq!(object.insert("10".into(), 4.into()), Some(1.into()));
        assert_eq!(
            object.keys().map(String::as_str).collect::<Vec<_>>(),
            ["10", "2", "name"]
        );
        *object.get_mut("2").unwrap() = 5.into();
        for (_, value) in &mut object {
            if value.as_f64() == Some(3.0) {
                *value = 6.into();
            }
        }
        assert_eq!(object.shift_remove("2"), Some(5.into()));
        object.extend([("new".to_owned(), 7.into())]);
        assert_eq!(
            object.into_iter().map(|(key, _)| key).collect::<Vec<_>>(),
            ["10", "name", "new"]
        );
    }
    #[test]
    fn construction_borrows_expressions_and_supports_current_style_macros() {
        let source = json!({"x":f64::NAN,"height":12});
        let values = vec![1.0, 2.0];
        let key = "dynamic";
        let result = json!({key:source["x"],"conditional":if true {1.5}else{0.0},"copy":values,"optional":Some("font"),"none":None::<&str>,"nested":[&source["height"],null,true,{"k":3},],});
        assert!(source["x"].as_f64().unwrap().is_nan());
        assert!(result[key].as_f64().unwrap().is_nan());
        assert_eq!(values.len(), 2);
        assert_eq!(result["conditional"], 1.5);
        assert_eq!(result["optional"], "font");
        assert!(result["none"].is_null());
        assert_eq!(result["nested"][0], 12);
        let empty_object = json!({});
        let empty_array = json!([]);
        assert!(empty_object.as_object().unwrap().is_empty());
        assert!(empty_array.as_array().unwrap().is_empty());
        let converted = Value::from_json(&serde_json::json!({"text":"한😀","a":[1,null,false]}));
        assert_eq!(
            converted.to_json(),
            serde_json::json!({"text":"한😀","a":[1.0,null,false]})
        );
    }
    #[test]
    fn integer_accessors_reject_nonfinite_fractional_and_out_of_range_values() {
        assert_eq!(Value::from(-0.0).as_u64(), Some(0));
        assert_eq!(Value::from(-3).as_i64(), Some(-3));
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1.5] {
            assert_eq!(Value::from(value).as_i64(), None);
            assert_eq!(Value::from(value).as_u64(), None);
        }
        assert_eq!(Value::from(9223372036854775808.0).as_i64(), None);
        assert_eq!(Value::from(18446744073709551616.0).as_u64(), None);
    }
    #[test]
    fn default_string_conversion_retains_nested_nonfinite_array_values() {
        let value = json!([
            NativeValue::Undefined,
            null,
            [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.0],
            {}
        ]);
        assert_eq!(string(&value), ",,NaN,Infinity,-Infinity,0,[object Object]");
        assert_eq!(string(&NativeValue::Undefined), "undefined");
        assert_eq!(string(&NativeValue::Null), "null");
        assert_eq!(string(&json!(true)), "true");
        assert_eq!(string(&json!(1e21)), "1e+21");
        assert_eq!(string(&json!(1e-7)), "1e-7");
        assert_eq!(
            string(&json!({"toString":"custom","valueOf":12})),
            "[object Object]"
        );
        assert_eq!(string(&json!([[], [NativeValue::Undefined, null]])), ",,");
    }
    #[test]
    fn native_number_conversion_does_not_json_normalize_arrays() {
        assert!(number(&json!([f64::NAN])).is_nan());
        assert_eq!(number(&json!([f64::INFINITY])), f64::INFINITY);
        assert_eq!(number(&json!([[f64::NEG_INFINITY]])), f64::NEG_INFINITY);
        assert_eq!(number(&json!([[NativeValue::Undefined]])), 0.0);
        assert_eq!(number(&json!([null])), 0.0);
        assert_eq!(number(&json!([])), 0.0);
        assert!(number(&json!([null, NativeValue::Undefined])).is_nan());
        assert!(number(&json!({})).is_nan());
        assert!(number(&NativeValue::Undefined).is_nan());
        assert_eq!(number(&NativeValue::Null), 0.0);
        assert_eq!(number(&json!(true)), 1.0);
        assert_eq!(number(&json!(false)), 0.0);
        assert!(number(&json!(-0.0)).is_sign_negative());
        assert!(number(&json!([-0.0])).is_sign_positive());
        assert!(number(&json!("-0")).is_sign_negative());
        assert_eq!(number(&json!(["\u{feff}0x10\u{a0}"])), 16.0);
        assert_eq!(
            number(&json!(["0x2c6099f8f123456789abc"])),
            3.3530577209625013e24
        );
    }
}
