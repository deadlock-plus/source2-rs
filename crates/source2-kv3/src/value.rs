//! The KV3 value model: [`Value`], [`Kind`] and the flag bits.

use crate::Object;

/// Bits of a value's flag byte.
///
/// The flag byte tags a value as a reference to something outside the document. Several bits
/// may be set at once. Bits outside these five are kept in binary form but have no text
/// spelling, so the text writer refuses a value that carries one.
pub mod flag {
    /// A path to another resource.
    pub const RESOURCE: u8 = 1;
    /// The name of a resource.
    pub const RESOURCE_NAME: u8 = 2;
    /// A Panorama UI reference.
    pub const PANORAMA: u8 = 4;
    /// A sound event name.
    pub const SOUND_EVENT: u8 = 8;
    /// A subclass name.
    pub const SUBCLASS: u8 = 16;

    /// Every bit that has a text spelling.
    pub const SPELLED: u8 = RESOURCE | RESOURCE_NAME | PANORAMA | SOUND_EVENT | SUBCLASS;

    /// The text prefixes, one per bit, in bit order.
    pub(crate) const NAMES: [(u8, &str); 5] = [
        (RESOURCE, "resource"),
        (RESOURCE_NAME, "resource_name"),
        (PANORAMA, "panorama"),
        (SOUND_EVENT, "soundevent"),
        (SUBCLASS, "subclass"),
    ];
}

/// What a value holds.
///
/// Integers and floats are kept at full width whatever they were stored as, so a `Kind` never
/// depends on the encoding. How a number was stored is remembered separately by [`Value`].
///
/// `PartialEq` follows the contents: two `Double(NaN)` are not equal, as for `f64`.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// An explicit null.
    Null,
    /// A boolean.
    Bool(bool),
    /// Any signed integer width, widened.
    Int(i64),
    /// Any unsigned integer width, widened.
    UInt(u64),
    /// A float or double, widened.
    Double(f64),
    /// A string.
    String(String),
    /// An opaque binary blob.
    Blob(Vec<u8>),
    /// An ordered list.
    Array(Vec<Value>),
    /// A keyed collection.
    Object(Object),
}

/// How an array is laid out in the type stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ArrayForm {
    /// A 4-byte length, then a type code per element.
    General,
    /// A 4-byte length, then one type code for the whole array.
    Typed { ty: u8, flags: u8 },
    /// Like `Typed` with a 1-byte length.
    ByteLength { ty: u8, flags: u8 },
    /// Like `ByteLength`, with the elements' operands drawn from buffer 1.
    Auxiliary { ty: u8, flags: u8 },
}

/// How a document stored a value, where the value alone does not settle it.
///
/// Read from a file and kept so that writing the value back reproduces the file. The writer
/// only follows it when it can still hold the value exactly: after the value is changed, or
/// when the target revision cannot store the form, the writer falls back to its own choice.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Storage {
    #[default]
    Inferred,
    /// The type code a number, boolean or null was stored under.
    Scalar(u8),
    /// The layout of an array.
    Array(ArrayForm),
    /// A string written between `"""` in text.
    Multiline,
}

/// A KV3 value.
///
/// A value is a [`Kind`] plus a flag byte (see [`flag`]). Values read from a file also remember
/// how each number, array and multi-line string was stored - integer width, float against
/// double, typed against general arrays, `"""` against a quoted string - so writing the
/// document back reproduces the file. Values built by hand carry none of that and are written
/// the way Valve's files usually are.
///
/// # Storage hints
///
/// The hints are invisible: [`Debug`](std::fmt::Debug) leaves them out, `==` ignores them, and
/// [`Clone`] keeps them (a clone of a parsed tree writes the same bytes as the original). A
/// hint is only a preference. The writer follows it when it still holds the value exactly, so
/// editing a value through [`kind_mut`](Value::kind_mut) can never produce a block that
/// misstates it: an integer stored in a byte that grows to 300 is written wider, and an array
/// that gains an element of another type is written with a type code per element.
/// [`set_kind`](Value::set_kind) drops the hint outright, and
/// [`clear_storage`](Value::clear_storage) drops every hint under a value, which is how to ask
/// for Valve's default encoding of a parsed tree.
///
/// # Numbers
///
/// [`From<f32>`](Value#impl-From<f32>-for-Value) widens exactly and stores a double;
/// [`Value::float`] stores the 4-byte float type the same number fits in. Text has no float
/// width, so it is not kept through text.
#[derive(Clone)]
pub struct Value {
    kind: Kind,
    flags: u8,
    storage: Storage,
}

impl PartialEq for Value {
    /// Compares the kind and the flags, not the storage hints.
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind && self.flags == other.flags
    }
}

impl std::fmt::Debug for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = f.debug_struct("Value");
        s.field("kind", &self.kind);
        if self.flags != 0 {
            s.field("flags", &self.flags);
        }
        s.finish()
    }
}

impl Default for Value {
    /// [`Value::null`].
    fn default() -> Self {
        Value::null()
    }
}

impl From<Kind> for Value {
    fn from(kind: Kind) -> Self {
        Value {
            kind,
            flags: 0,
            storage: Storage::Inferred,
        }
    }
}

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Kind::Bool(v).into()
    }
}

macro_rules! from_signed {
    ($($t:ty),*) => {$(
        impl From<$t> for Value {
            fn from(v: $t) -> Self {
                Kind::Int(i64::from(v)).into()
            }
        }
    )*};
}
from_signed!(i8, i16, i32, i64);

macro_rules! from_unsigned {
    ($($t:ty),*) => {$(
        impl From<$t> for Value {
            fn from(v: $t) -> Self {
                Kind::UInt(u64::from(v)).into()
            }
        }
    )*};
}
from_unsigned!(u8, u16, u32, u64);

impl From<f32> for Value {
    /// Widens exactly and stores a double. See [`Value::float`] for the 4-byte type.
    fn from(v: f32) -> Self {
        Kind::Double(f64::from(v)).into()
    }
}

impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Kind::Double(v).into()
    }
}

impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Kind::String(v.to_string()).into()
    }
}

impl From<String> for Value {
    fn from(v: String) -> Self {
        Kind::String(v).into()
    }
}

impl From<Vec<Value>> for Value {
    fn from(v: Vec<Value>) -> Self {
        Kind::Array(v).into()
    }
}

impl From<Object> for Value {
    fn from(v: Object) -> Self {
        Kind::Object(v).into()
    }
}

impl FromIterator<Value> for Value {
    /// Collects into an array.
    fn from_iter<I: IntoIterator<Item = Value>>(iter: I) -> Self {
        Kind::Array(iter.into_iter().collect()).into()
    }
}

impl Value {
    /// An explicit null.
    #[must_use]
    pub fn null() -> Self {
        Kind::Null.into()
    }

    /// A signed integer.
    #[must_use]
    pub fn int(v: i64) -> Self {
        Kind::Int(v).into()
    }

    /// An unsigned integer.
    #[must_use]
    pub fn uint(v: u64) -> Self {
        Kind::UInt(v).into()
    }

    /// A double.
    #[must_use]
    pub fn double(v: f64) -> Self {
        Kind::Double(v).into()
    }

    /// A single-precision float, stored in the 4-byte float type.
    #[must_use]
    pub fn float(v: f32) -> Self {
        Value::stored(
            Kind::Double(f64::from(v)),
            0,
            Storage::Scalar(crate::node::FLOAT),
        )
    }

    /// A binary blob.
    #[must_use]
    pub fn blob(bytes: Vec<u8>) -> Self {
        Kind::Blob(bytes).into()
    }

    /// An array.
    #[must_use]
    pub fn array(items: Vec<Value>) -> Self {
        Kind::Array(items).into()
    }

    /// A string that text KV3 writes between `"""` rather than in quotes.
    ///
    /// Binary KV3 has no such distinction. The text writer falls back to the quoted form when
    /// the string cannot sit between `"""` unchanged: when it contains `"""` or ends in a
    /// carriage return.
    #[must_use]
    pub fn multiline(text: impl Into<String>) -> Self {
        Value::stored(Kind::String(text.into()), 0, Storage::Multiline)
    }

    pub(crate) fn stored(kind: Kind, flags: u8, storage: Storage) -> Self {
        Value {
            kind,
            flags,
            storage,
        }
    }

    pub(crate) fn storage(&self) -> Storage {
        self.storage
    }

    /// What the value holds.
    #[must_use]
    pub fn kind(&self) -> &Kind {
        &self.kind
    }

    /// Mutable access to what the value holds.
    ///
    /// The storage hint stays, and the writer only follows it while it still holds the edited
    /// value; see the [type docs](Value#storage-hints).
    pub fn kind_mut(&mut self) -> &mut Kind {
        &mut self.kind
    }

    /// Replace what the value holds, dropping the storage hint.
    pub fn set_kind(&mut self, kind: Kind) {
        self.kind = kind;
        self.storage = Storage::Inferred;
    }

    /// Take the contents, dropping the flags and the hint.
    #[must_use]
    pub fn into_kind(self) -> Kind {
        self.kind
    }

    /// Forget how this value and everything under it was stored, so the writer chooses.
    pub fn clear_storage(&mut self) {
        self.storage = Storage::Inferred;
        match &mut self.kind {
            Kind::Array(items) => items.iter_mut().for_each(Value::clear_storage),
            Kind::Object(o) => o.iter_mut().for_each(|(_, v)| v.clear_storage()),
            _ => {}
        }
    }

    /// The flag byte: a combination of the [`flag`] bits, `0` for a plain value.
    #[must_use]
    pub fn flags(&self) -> u8 {
        self.flags
    }

    /// Replace the flag byte.
    pub fn set_flags(&mut self, flags: u8) {
        self.flags = flags;
    }

    /// The same value with the given flag byte.
    #[must_use]
    pub fn with_flags(mut self, flags: u8) -> Self {
        self.flags = flags;
        self
    }

    /// Whether this is [`Kind::Null`].
    #[must_use]
    pub fn is_null(&self) -> bool {
        matches!(self.kind, Kind::Null)
    }

    /// Whether this string is written between `"""` in text.
    #[must_use]
    pub fn is_multiline(&self) -> bool {
        self.storage == Storage::Multiline && matches!(self.kind, Kind::String(_))
    }

    /// The string, if this is one.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match &self.kind {
            Kind::String(s) => Some(s),
            _ => None,
        }
    }

    /// The bytes, if this is a blob.
    #[must_use]
    pub fn as_blob(&self) -> Option<&[u8]> {
        match &self.kind {
            Kind::Blob(b) => Some(b),
            _ => None,
        }
    }

    /// An integer as `i64`, whichever width or signedness it was stored as. Exact: an unsigned
    /// value above `i64::MAX` is `None`, and a float is `None` even if it is whole.
    ///
    /// The format stores a number in whatever encoding is smallest, so the same field can
    /// arrive as a byte in one file and an unsigned 32-bit in another. Callers care about the
    /// number, not the encoding.
    #[must_use]
    pub fn as_i64(&self) -> Option<i64> {
        match &self.kind {
            Kind::Int(v) => Some(*v),
            Kind::UInt(v) => i64::try_from(*v).ok(),
            _ => None,
        }
    }

    /// An integer as `u64`. Exact: a negative value is `None`.
    #[must_use]
    pub fn as_u64(&self) -> Option<u64> {
        match &self.kind {
            Kind::Int(v) => u64::try_from(*v).ok(),
            Kind::UInt(v) => Some(*v),
            _ => None,
        }
    }

    /// An integer as `u32`, when it fits.
    #[must_use]
    pub fn as_u32(&self) -> Option<u32> {
        u32::try_from(self.as_u64()?).ok()
    }

    /// The boolean, if this is one.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match &self.kind {
            Kind::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// The number as `f64`. Exact: a double as it is, and an integer only when an `f64` holds
    /// it without rounding (magnitude up to 2^53). Use [`to_f64_lossy`](Value::to_f64_lossy)
    /// to accept every integer.
    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        const EXACT: u64 = 1 << 53;
        match &self.kind {
            Kind::Double(d) => Some(*d),
            Kind::Int(v) if v.unsigned_abs() <= EXACT => Some(*v as f64),
            Kind::UInt(v) if *v <= EXACT => Some(*v as f64),
            _ => None,
        }
    }

    /// The number as `f64`, rounding integers that an `f64` cannot hold exactly.
    #[must_use]
    pub fn to_f64_lossy(&self) -> Option<f64> {
        match &self.kind {
            Kind::Double(d) => Some(*d),
            Kind::Int(v) => Some(*v as f64),
            Kind::UInt(v) => Some(*v as f64),
            _ => None,
        }
    }

    /// The number as `f32`, rounding anything that `f32` cannot hold exactly.
    #[must_use]
    pub fn to_f32_lossy(&self) -> Option<f32> {
        self.to_f64_lossy().map(|d| d as f32)
    }

    /// The elements, if this is an array.
    #[must_use]
    pub fn as_array(&self) -> Option<&[Value]> {
        match &self.kind {
            Kind::Array(v) => Some(v),
            _ => None,
        }
    }

    /// The elements for editing, if this is an array.
    pub fn as_array_mut(&mut self) -> Option<&mut Vec<Value>> {
        match &mut self.kind {
            Kind::Array(v) => Some(v),
            _ => None,
        }
    }

    /// The object, if this is one.
    #[must_use]
    pub fn as_object(&self) -> Option<&Object> {
        match &self.kind {
            Kind::Object(o) => Some(o),
            _ => None,
        }
    }

    /// The object for editing, if this is one.
    pub fn as_object_mut(&mut self) -> Option<&mut Object> {
        match &mut self.kind {
            Kind::Object(o) => Some(o),
            _ => None,
        }
    }

    /// Look up a member, if this is an object.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.as_object()?.get(key)
    }

    /// Look up a member for editing, if this is an object.
    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        self.as_object_mut()?.get_mut(key)
    }
}
