//! [`Object`]: an ordered keyed collection.

use std::collections::HashMap;
use std::ops::Index;

use crate::Value;

/// A keyed collection that keeps the order its members were added in.
///
/// KV3 allows a key to repeat, and real files do occasionally repeat one, so an `Object` is a
/// list of members with a lookup index over it rather than a map. Lookups
/// ([`get`](Object::get), [`get_mut`](Object::get_mut), [`remove`](Object::remove), indexing)
/// act on the first member with a key; [`get_all`](Object::get_all) sees every one. The index is
/// private and is kept consistent through every method here, including after removals.
///
/// Two ways to add a member, deliberately different:
///
/// - [`insert`](Object::insert) behaves as for a map: an existing key has its value replaced
///   (and the old one returned), a new key is appended.
/// - [`push`](Object::push), [`Extend`], [`FromIterator`] and [`From<Vec<(K, Value)>>`] append
///   every pair as given, repeated keys included. That is what reading a file does, and what
///   rebuilding an object from another one's members needs.
///
/// Equality compares members in order.
#[derive(Clone, Default)]
pub struct Object {
    entries: Vec<(String, Value)>,
    index: HashMap<String, usize>,
}

impl std::fmt::Debug for Object {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

impl PartialEq for Object {
    fn eq(&self, other: &Self) -> bool {
        self.entries == other.entries
    }
}

impl Object {
    /// An empty object.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Look up a member by name; the first, where a key repeats.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.index.get(key).map(|&i| &self.entries[i].1)
    }

    /// Look up a member by name for editing; the first, where a key repeats.
    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        let i = *self.index.get(key)?;
        Some(&mut self.entries[i].1)
    }

    /// Every member with this name, in order.
    pub fn get_all<'a>(&'a self, key: &'a str) -> impl Iterator<Item = &'a Value> {
        self.entries
            .iter()
            .filter(move |(k, _)| k == key)
            .map(|(_, v)| v)
    }

    /// Whether any member has this name.
    #[must_use]
    pub fn contains_key(&self, key: &str) -> bool {
        self.index.contains_key(key)
    }

    /// Members in order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&str, &Value)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Members in order, with their values open to editing. Names cannot change, so the lookup
    /// index stays valid.
    pub fn iter_mut(&mut self) -> impl ExactSizeIterator<Item = (&str, &mut Value)> {
        self.entries.iter_mut().map(|(k, v)| (k.as_str(), v))
    }

    /// Member names in order.
    pub fn keys(&self) -> impl ExactSizeIterator<Item = &str> {
        self.entries.iter().map(|(k, _)| k.as_str())
    }

    /// Member values in order.
    pub fn values(&self) -> impl ExactSizeIterator<Item = &Value> {
        self.entries.iter().map(|(_, v)| v)
    }

    /// How many members there are, repeated keys counted each time.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether there are no members.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Set a member: replace the first member with this name and return its old value, or
    /// append a new one.
    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<Value>) -> Option<Value> {
        let key = key.into();
        let value = value.into();
        match self.index.get(&key) {
            Some(&i) => Some(std::mem::replace(&mut self.entries[i].1, value)),
            None => {
                self.index.insert(key.clone(), self.entries.len());
                self.entries.push((key, value));
                None
            }
        }
    }

    /// Append a member without looking at the existing ones, so a repeated key is kept.
    pub fn push(&mut self, key: impl Into<String>, value: impl Into<Value>) {
        let key = key.into();
        self.index.entry(key.clone()).or_insert(self.entries.len());
        self.entries.push((key, value.into()));
    }

    /// Remove the first member with this name and return its value. Later members keep their
    /// order, and a later member with the same name becomes the one lookups find.
    pub fn remove(&mut self, key: &str) -> Option<Value> {
        let at = self.index.get(key).copied()?;
        let (_, value) = self.entries.remove(at);
        self.reindex();
        Some(value)
    }

    /// Remove every member.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.index.clear();
    }

    fn reindex(&mut self) {
        self.index.clear();
        for (i, (k, _)) in self.entries.iter().enumerate() {
            self.index.entry(k.clone()).or_insert(i);
        }
    }
}

impl Index<&str> for Object {
    type Output = Value;

    /// The first member with this name.
    ///
    /// # Panics
    ///
    /// If there is none. Use [`Object::get`] for a fallible lookup.
    fn index(&self, key: &str) -> &Value {
        self.get(key)
            .unwrap_or_else(|| panic!("no member named {key:?} in the object"))
    }
}

impl<K: Into<String>, V: Into<Value>> Extend<(K, V)> for Object {
    /// Appends every pair, keeping repeated keys; see [`Object::push`].
    fn extend<I: IntoIterator<Item = (K, V)>>(&mut self, iter: I) {
        for (k, v) in iter {
            self.push(k, v);
        }
    }
}

impl<K: Into<String>, V: Into<Value>> FromIterator<(K, V)> for Object {
    /// Appends every pair, keeping repeated keys; see [`Object::push`].
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        let mut o = Object::new();
        o.extend(iter);
        o
    }
}

impl<K: Into<String>> From<Vec<(K, Value)>> for Object {
    fn from(pairs: Vec<(K, Value)>) -> Self {
        pairs.into_iter().collect()
    }
}

impl IntoIterator for Object {
    type Item = (String, Value);
    type IntoIter = std::vec::IntoIter<(String, Value)>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_iter()
    }
}

impl<'a> IntoIterator for &'a Object {
    type Item = (&'a str, &'a Value);
    type IntoIter = std::iter::Map<
        std::slice::Iter<'a, (String, Value)>,
        fn(&'a (String, Value)) -> (&'a str, &'a Value),
    >;

    fn into_iter(self) -> Self::IntoIter {
        self.entries
            .iter()
            .map(pair as fn(&'a (String, Value)) -> (&'a str, &'a Value))
    }
}

impl<'a> IntoIterator for &'a mut Object {
    type Item = (&'a str, &'a mut Value);
    type IntoIter = std::iter::Map<
        std::slice::IterMut<'a, (String, Value)>,
        fn(&'a mut (String, Value)) -> (&'a str, &'a mut Value),
    >;

    fn into_iter(self) -> Self::IntoIter {
        self.entries
            .iter_mut()
            .map(pair_mut as fn(&'a mut (String, Value)) -> (&'a str, &'a mut Value))
    }
}

fn pair(e: &(String, Value)) -> (&str, &Value) {
    (e.0.as_str(), &e.1)
}

fn pair_mut(e: &mut (String, Value)) -> (&str, &mut Value) {
    (e.0.as_str(), &mut e.1)
}
