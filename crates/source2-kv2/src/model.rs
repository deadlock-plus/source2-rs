//! The in-memory Datamodel.

use crate::Uuid;

/// Index of an element in [`Document::elements`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ElementId(pub u32);

/// How a document is serialized.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Encoding {
    /// `binary`.
    Binary,
    /// `keyvalues2`.
    KeyValues2,
    /// `keyvalues2_noids`: text with no element ids, so nothing can be shared.
    KeyValues2NoIds,
}

impl Encoding {
    /// The name used in the header line.
    pub fn name(self) -> &'static str {
        match self {
            Encoding::Binary => "binary",
            Encoding::KeyValues2 => "keyvalues2",
            Encoding::KeyValues2NoIds => "keyvalues2_noids",
        }
    }

    /// Looks an encoding up by its header name.
    pub fn from_name(name: &str) -> Option<Encoding> {
        match name {
            "binary" => Some(Encoding::Binary),
            "keyvalues2" => Some(Encoding::KeyValues2),
            "keyvalues2_noids" => Some(Encoding::KeyValues2NoIds),
            _ => None,
        }
    }
}

/// A time in ticks of 1/10000 second, as DMX stores it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Time(pub i32);

/// An RGBA color.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Color {
    /// Red.
    pub r: u8,
    /// Green.
    pub g: u8,
    /// Blue.
    pub b: u8,
    /// Alpha.
    pub a: u8,
}

/// What an element attribute points at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ElementRef {
    /// No element.
    Null,
    /// An element of the same document.
    Element(ElementId),
    /// An id with no element in this document. The binary encodings can store this
    /// directly; the text readers produce it only when asked to tolerate dangling ids.
    External(Uuid),
}

/// The type of one attribute value, or of each item of an array.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ValueType {
    /// An [`ElementRef`].
    Element,
    /// A 32-bit signed integer.
    Int,
    /// A 32-bit float.
    Float,
    /// A boolean.
    Bool,
    /// A string.
    String,
    /// Raw bytes.
    Binary,
    /// A bare id. Binary versions 1 and 2 only.
    ObjectId,
    /// A [`Time`].
    Time,
    /// A [`Color`].
    Color,
    /// Two floats.
    Vector2,
    /// Three floats.
    Vector3,
    /// Four floats.
    Vector4,
    /// Three floats, Euler angles.
    QAngle,
    /// Four floats.
    Quaternion,
    /// Sixteen floats, row-major.
    Matrix,
    /// A 64-bit unsigned integer. Source 2 binary version 9 and text only.
    UInt64,
    /// A byte. Source 2 binary version 9 and text only.
    UInt8,
}

/// One attribute value.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// See [`ValueType::Element`].
    Element(ElementRef),
    /// See [`ValueType::Int`].
    Int(i32),
    /// See [`ValueType::Float`].
    Float(f32),
    /// See [`ValueType::Bool`].
    Bool(bool),
    /// See [`ValueType::String`].
    String(String),
    /// See [`ValueType::Binary`].
    Binary(Vec<u8>),
    /// See [`ValueType::ObjectId`].
    ObjectId(Uuid),
    /// See [`ValueType::Time`].
    Time(Time),
    /// See [`ValueType::Color`].
    Color(Color),
    /// See [`ValueType::Vector2`].
    Vector2([f32; 2]),
    /// See [`ValueType::Vector3`].
    Vector3([f32; 3]),
    /// See [`ValueType::Vector4`].
    Vector4([f32; 4]),
    /// See [`ValueType::QAngle`].
    QAngle([f32; 3]),
    /// See [`ValueType::Quaternion`].
    Quaternion([f32; 4]),
    /// See [`ValueType::Matrix`].
    Matrix([f32; 16]),
    /// See [`ValueType::UInt64`].
    UInt64(u64),
    /// See [`ValueType::UInt8`].
    UInt8(u8),
    /// An array. Every item must be a scalar of the given type; the type is carried so an
    /// empty array still knows what it holds.
    Array(ValueType, Vec<Value>),
}

impl Value {
    /// The scalar type of this value, or the item type for an array.
    pub fn value_type(&self) -> ValueType {
        match self {
            Value::Element(_) => ValueType::Element,
            Value::Int(_) => ValueType::Int,
            Value::Float(_) => ValueType::Float,
            Value::Bool(_) => ValueType::Bool,
            Value::String(_) => ValueType::String,
            Value::Binary(_) => ValueType::Binary,
            Value::ObjectId(_) => ValueType::ObjectId,
            Value::Time(_) => ValueType::Time,
            Value::Color(_) => ValueType::Color,
            Value::Vector2(_) => ValueType::Vector2,
            Value::Vector3(_) => ValueType::Vector3,
            Value::Vector4(_) => ValueType::Vector4,
            Value::QAngle(_) => ValueType::QAngle,
            Value::Quaternion(_) => ValueType::Quaternion,
            Value::Matrix(_) => ValueType::Matrix,
            Value::UInt64(_) => ValueType::UInt64,
            Value::UInt8(_) => ValueType::UInt8,
            Value::Array(t, _) => *t,
        }
    }

    /// Whether this is an array.
    pub fn is_array(&self) -> bool {
        matches!(self, Value::Array(..))
    }
}

/// A named value on an element.
#[derive(Clone, Debug, PartialEq)]
pub struct Attribute {
    /// Attribute name.
    pub name: String,
    /// Attribute value.
    pub value: Value,
}

impl Attribute {
    /// Builds an attribute.
    pub fn new(name: impl Into<String>, value: Value) -> Self {
        Attribute {
            name: name.into(),
            value,
        }
    }
}

/// One node of the element graph.
///
/// The `id` and `name` attributes every element has live in their own fields and are not
/// repeated in [`Element::attributes`].
#[derive(Clone, Debug, PartialEq)]
pub struct Element {
    /// Class name, e.g. `DmElement`.
    pub class: String,
    /// Element name; empty when unnamed.
    pub name: String,
    /// Stable id other elements refer to.
    pub id: Uuid,
    /// Remaining attributes, in document order.
    pub attributes: Vec<Attribute>,
}

impl Element {
    /// Builds an element with no attributes.
    pub fn new(class: impl Into<String>, name: impl Into<String>, id: Uuid) -> Self {
        Element {
            class: class.into(),
            name: name.into(),
            id,
            attributes: Vec::new(),
        }
    }

    /// The first attribute with this name.
    pub fn attribute(&self, name: &str) -> Option<&Value> {
        self.attributes
            .iter()
            .find(|a| a.name == name)
            .map(|a| &a.value)
    }
}

/// A whole Datamodel document.
///
/// Elements sit in one arena and refer to each other by [`ElementId`], so shared elements
/// and cycles need no reference counting. `elements[0]` is the root.
#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    /// How the document is serialized.
    pub encoding: Encoding,
    /// Version of that encoding.
    pub encoding_version: u32,
    /// Format name, e.g. `dmx` or `vmap`.
    pub format: String,
    /// Version of that format.
    pub format_version: u32,
    /// Prefix elements: attribute lists that precede the root. Source 2 writes one.
    pub prefix: Vec<Vec<Attribute>>,
    /// Every element; the root first.
    pub elements: Vec<Element>,
}

impl Document {
    /// An empty document.
    pub fn new(
        encoding: Encoding,
        encoding_version: u32,
        format: impl Into<String>,
        format_version: u32,
    ) -> Self {
        Document {
            encoding,
            encoding_version,
            format: format.into(),
            format_version,
            prefix: Vec::new(),
            elements: Vec::new(),
        }
    }

    /// The root element, if the document has any element.
    pub fn root(&self) -> Option<&Element> {
        self.elements.first()
    }

    /// The element an id points at.
    pub fn element(&self, id: ElementId) -> Option<&Element> {
        self.elements.get(id.0 as usize)
    }

    /// Adds an element and returns its id.
    ///
    /// # Panics
    ///
    /// Panics if the document already holds `u32::MAX` elements.
    pub fn add_element(&mut self, element: Element) -> ElementId {
        let id = ElementId(u32::try_from(self.elements.len()).expect("element count fits u32"));
        self.elements.push(element);
        id
    }

    /// The element with this id, if any.
    pub fn find_by_id(&self, id: &Uuid) -> Option<ElementId> {
        self.elements
            .iter()
            .position(|e| e.id == *id)
            .and_then(|i| u32::try_from(i).ok())
            .map(ElementId)
    }
}
