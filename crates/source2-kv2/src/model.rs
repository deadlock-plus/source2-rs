//! The in-memory Datamodel.

use crate::Uuid;

/// Index of an element in [`Document::elements`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ElementId(pub u32);

/// How a document is serialized.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
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
    /// An id with no element in this document, kept as written. See
    /// [`Document::check_references`].
    External(Uuid),
}

impl From<ElementId> for ElementRef {
    fn from(id: ElementId) -> Self {
        ElementRef::Element(id)
    }
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

impl From<i32> for Value {
    fn from(v: i32) -> Self {
        Value::Int(v)
    }
}

impl From<f32> for Value {
    fn from(v: f32) -> Self {
        Value::Float(v)
    }
}

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::Bool(v)
    }
}

impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::String(v.to_string())
    }
}

impl From<String> for Value {
    fn from(v: String) -> Self {
        Value::String(v)
    }
}

impl From<Vec<u8>> for Value {
    fn from(v: Vec<u8>) -> Self {
        Value::Binary(v)
    }
}

impl From<ElementId> for Value {
    fn from(v: ElementId) -> Self {
        Value::Element(ElementRef::Element(v))
    }
}

impl From<ElementRef> for Value {
    fn from(v: ElementRef) -> Self {
        Value::Element(v)
    }
}

impl From<Time> for Value {
    fn from(v: Time) -> Self {
        Value::Time(v)
    }
}

impl From<Color> for Value {
    fn from(v: Color) -> Self {
        Value::Color(v)
    }
}

impl From<[f32; 2]> for Value {
    fn from(v: [f32; 2]) -> Self {
        Value::Vector2(v)
    }
}

impl From<[f32; 3]> for Value {
    fn from(v: [f32; 3]) -> Self {
        Value::Vector3(v)
    }
}

impl From<[f32; 4]> for Value {
    fn from(v: [f32; 4]) -> Self {
        Value::Vector4(v)
    }
}

impl From<[f32; 16]> for Value {
    fn from(v: [f32; 16]) -> Self {
        Value::Matrix(v)
    }
}

impl From<u64> for Value {
    fn from(v: u64) -> Self {
        Value::UInt64(v)
    }
}

impl From<u8> for Value {
    fn from(v: u8) -> Self {
        Value::UInt8(v)
    }
}

impl Value {
    /// An array of `ty` holding `items`. Use this for an empty array too.
    pub fn array<T: Into<Value>>(ty: ValueType, items: impl IntoIterator<Item = T>) -> Value {
        Value::Array(ty, items.into_iter().map(Into::into).collect())
    }

    /// Euler angles; `[f32; 3]` converts to a [`Value::Vector3`] instead.
    pub fn qangle(v: [f32; 3]) -> Value {
        Value::QAngle(v)
    }

    /// A quaternion; `[f32; 4]` converts to a [`Value::Vector4`] instead.
    pub fn quaternion(v: [f32; 4]) -> Value {
        Value::Quaternion(v)
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
    pub fn new(name: impl Into<String>, value: impl Into<Value>) -> Self {
        Attribute {
            name: name.into(),
            value: value.into(),
        }
    }
}

fn set_attribute(attrs: &mut Vec<Attribute>, name: String, value: Value) {
    match attrs.iter_mut().find(|a| a.name == name) {
        Some(a) => a.value = value,
        None => attrs.push(Attribute { name, value }),
    }
}

/// How a text document lays one element out. Hand-built elements keep the default.
///
/// Text puts `id` and `name` among the attributes, so where they sit is part of the file.
/// Positions count every line of the element body, `id` and `name` included.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextLayout {
    /// Position of the `id` line. `0` puts it first.
    pub id_position: usize,
    /// Position of the `name` line. `None` puts it right after `id` and leaves it out
    /// when the name is empty; `Some` always writes it, even for an empty name.
    pub name_position: Option<usize>,
    /// Written as its own top-level block and referred to by id everywhere, rather than
    /// nested where it is first used. Readers set this for every top-level block. The
    /// first element is always written top-level.
    pub standalone: bool,
}

/// One node of the element graph.
///
/// The `id` and `name` every element has live in their own fields and are not repeated in
/// [`Element::attributes`]. In text, `id` and `name` are reserved attribute names.
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
    /// Where text puts this element's lines. Ignored by the binary encodings.
    pub text: TextLayout,
}

impl Element {
    /// An unnamed element of this class with a fresh id and no attributes.
    pub fn new(class: impl Into<String>) -> Self {
        Element::from_parts(class, "", Uuid::generate())
    }

    /// An element with every identifying field given.
    pub fn from_parts(class: impl Into<String>, name: impl Into<String>, id: Uuid) -> Self {
        Element {
            class: class.into(),
            name: name.into(),
            id,
            attributes: Vec::new(),
            text: TextLayout::default(),
        }
    }

    /// Sets the name.
    #[must_use]
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Sets the id.
    #[must_use]
    pub fn id(mut self, id: Uuid) -> Self {
        self.id = id;
        self
    }

    /// Sets an attribute, replacing one of the same name or appending a new one.
    #[must_use]
    pub fn attr(mut self, name: impl Into<String>, value: impl Into<Value>) -> Self {
        self.set(name, value);
        self
    }

    /// Appends `item` to the array attribute `name`, creating it when absent.
    ///
    /// # Panics
    ///
    /// Panics if `name` exists and is not an array of `item`'s type, or `item` is itself
    /// an array.
    #[must_use]
    pub fn push(mut self, name: impl Into<String>, item: impl Into<Value>) -> Self {
        self.append(name.into(), item.into());
        self
    }

    fn append(&mut self, name: String, item: Value) {
        let ty = item.value_type();
        assert!(!item.is_array(), "arrays cannot nest");
        match self.attributes.iter_mut().find(|a| a.name == name) {
            Some(Attribute {
                value: Value::Array(t, items),
                ..
            }) if *t == ty => items.push(item),
            Some(_) => panic!("attribute `{name}` is not an array of {ty:?}"),
            None => self
                .attributes
                .push(Attribute::new(name, Value::Array(ty, vec![item]))),
        }
    }

    /// Sets an attribute, replacing one of the same name or appending a new one.
    pub fn set(&mut self, name: impl Into<String>, value: impl Into<Value>) {
        set_attribute(&mut self.attributes, name.into(), value.into());
    }

    /// The first attribute with this name.
    pub fn attribute(&self, name: &str) -> Option<&Value> {
        self.attributes
            .iter()
            .find(|a| a.name == name)
            .map(|a| &a.value)
    }
}

/// A prefix element: an attribute list that precedes the root. Source 2 writes one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Prefix {
    /// Its id. Text stores it; the binary encodings do not.
    pub id: Option<Uuid>,
    /// Its attributes. None may hold an element.
    pub attributes: Vec<Attribute>,
}

impl Prefix {
    /// An empty prefix element.
    pub fn new() -> Self {
        Prefix::default()
    }

    /// Sets an attribute, replacing one of the same name or appending a new one.
    #[must_use]
    pub fn attr(mut self, name: impl Into<String>, value: impl Into<Value>) -> Self {
        set_attribute(&mut self.attributes, name.into(), value.into());
        self
    }
}

/// Line ending of a text document.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Newline {
    /// `\n`.
    #[default]
    Lf,
    /// `\r\n`.
    CrLf,
}

impl Newline {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Newline::Lf => "\n",
            Newline::CrLf => "\r\n",
        }
    }
}

/// How text spells floats.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FloatFormat {
    /// The shortest decimal that reads back as the same `f32`. Lossless.
    #[default]
    Shortest,
    /// Ten decimals with trailing zeros dropped (`0.5`, `-0`, `10.0524339676`), as some of
    /// Valve's serializers print. Values below 5e-11 in magnitude become `0` or `-0`, so
    /// this is not lossless for tiny numbers.
    Fixed10,
}

/// Formatting habits of a text document. The defaults match Valve's serializers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextStyle {
    /// Line ending, header line included.
    pub newline: Newline,
    /// A blank line after an element nested as an attribute value.
    pub blank_line_after_element: bool,
    /// A blank line after each top-level block.
    pub blank_line_after_block: bool,
    /// A space after the type word of an array attribute that continues on the next line,
    /// e.g. `"x" "element_array" `.
    pub space_after_array_type: bool,
    /// Arrays with no nested element blocks on one line: `"x" "int_array" [ "1", "2" ]`,
    /// and `[ ]` when empty. Otherwise each item gets a line.
    pub inline_arrays: bool,
    /// A space after the comma that ends an array item's line.
    pub space_after_comma: bool,
    /// How floats are spelled.
    pub float_format: FloatFormat,
    /// The last line ends with a line break. Some files stop right after the closing brace.
    pub final_newline: bool,
}

impl Default for TextStyle {
    fn default() -> Self {
        TextStyle {
            newline: Newline::Lf,
            blank_line_after_element: true,
            blank_line_after_block: true,
            space_after_array_type: true,
            inline_arrays: false,
            space_after_comma: false,
            float_format: FloatFormat::Shortest,
            final_newline: true,
        }
    }
}

/// A whole Datamodel document.
///
/// Elements sit in one arena and refer to each other by [`ElementId`], so shared elements
/// and cycles need no reference counting. `elements[0]` is the root.
///
/// What a file stores beyond the elements (string table order, text whitespace, `id` and
/// `name` positions) has a public field with a default. A document built by hand writes
/// in Valve's conventions; a parsed one writes back as it was read.
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
    /// Prefix elements, before the root.
    pub prefix: Vec<Prefix>,
    /// Every element; the root first.
    pub elements: Vec<Element>,
    /// Binary string table, in file order (versions 2 and up). Strings it lacks are
    /// appended in first-use order when writing, so an empty table is fine.
    pub string_table: Vec<String>,
    /// Text whitespace.
    pub text_style: TextStyle,
}

impl Default for Document {
    /// An empty `dmx` version 1 document in `keyvalues2` version 1.
    fn default() -> Self {
        Document::new("dmx", 1)
    }
}

impl Document {
    /// An empty `keyvalues2` text document of the given format.
    pub fn new(format: impl Into<String>, format_version: u32) -> Self {
        Document::with_encoding(Encoding::KeyValues2, 1, format, format_version)
    }

    /// An empty document in a chosen encoding.
    pub fn with_encoding(
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
            string_table: Vec::new(),
            text_style: TextStyle::default(),
        }
    }

    /// The root element, if the document has any element.
    pub fn root(&self) -> Option<&Element> {
        self.elements.first()
    }

    /// The root element, mutably.
    pub fn root_mut(&mut self) -> Option<&mut Element> {
        self.elements.first_mut()
    }

    /// The element an id points at.
    pub fn element(&self, id: ElementId) -> Option<&Element> {
        self.elements.get(id.0 as usize)
    }

    /// The element an id points at, mutably.
    pub fn element_mut(&mut self, id: ElementId) -> Option<&mut Element> {
        self.elements.get_mut(id.0 as usize)
    }

    /// Adds an element and returns its id. The first element added is the root.
    ///
    /// # Panics
    ///
    /// Panics if the document already holds `u32::MAX` elements.
    pub fn add_element(&mut self, element: Element) -> ElementId {
        let id = ElementId(u32::try_from(self.elements.len()).expect("element count fits u32"));
        self.elements.push(element);
        id
    }

    /// Adds `child` and makes it the value of `parent`'s attribute `attr`.
    ///
    /// # Panics
    ///
    /// Panics if `parent` is not an element of this document.
    pub fn add_child(
        &mut self,
        parent: ElementId,
        attr: impl Into<String>,
        child: Element,
    ) -> ElementId {
        let id = self.add_element(child);
        self.link(parent, attr, id);
        id
    }

    /// Adds `child` and appends it to `parent`'s element array `attr`, creating the array
    /// when absent.
    ///
    /// # Panics
    ///
    /// Panics if `parent` is not an element of this document, or `attr` exists and is not
    /// an element array.
    pub fn push_child(
        &mut self,
        parent: ElementId,
        attr: impl Into<String>,
        child: Element,
    ) -> ElementId {
        let id = self.add_element(child);
        self.push_link(parent, attr, id);
        id
    }

    /// Makes `target` the value of `from`'s attribute `attr`.
    ///
    /// # Panics
    ///
    /// Panics if `from` is not an element of this document.
    pub fn link(&mut self, from: ElementId, attr: impl Into<String>, target: ElementId) {
        self.elements[from.0 as usize].set(attr, target);
    }

    /// Appends `target` to `from`'s element array `attr`, creating the array when absent.
    ///
    /// # Panics
    ///
    /// Panics if `from` is not an element of this document, or `attr` exists and is not an
    /// element array.
    pub fn push_link(&mut self, from: ElementId, attr: impl Into<String>, target: ElementId) {
        self.elements[from.0 as usize].append(attr.into(), target.into());
    }

    /// The element with this id, if any.
    pub fn find_by_id(&self, id: &Uuid) -> Option<ElementId> {
        self.elements
            .iter()
            .position(|e| e.id == *id)
            .and_then(|i| u32::try_from(i).ok())
            .map(ElementId)
    }

    /// Fails with [`crate::Error::UnresolvedReference`] on the first reference to an id
    /// that no element of the document has. Parsing keeps such references as
    /// [`ElementRef::External`]; call this when they should be an error.
    pub fn check_references(&self) -> crate::Result<()> {
        let dangling = |v: &Value| match v {
            Value::Element(ElementRef::External(u)) if self.find_by_id(u).is_none() => Some(*u),
            _ => None,
        };
        for e in &self.elements {
            for a in &e.attributes {
                let found = match &a.value {
                    Value::Array(_, items) => items.iter().find_map(dangling),
                    v => dangling(v),
                };
                if let Some(u) = found {
                    return Err(crate::Error::UnresolvedReference(u));
                }
            }
        }
        Ok(())
    }
}
