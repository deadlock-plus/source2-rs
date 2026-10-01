//! Binary KV3 type codes, and how a scalar maps to and from its operand.

use crate::value::{Kind, Storage, Value};

pub(crate) const NULL: u8 = 1;
pub(crate) const BOOLEAN: u8 = 2;
pub(crate) const INT64: u8 = 3;
pub(crate) const UINT64: u8 = 4;
pub(crate) const DOUBLE: u8 = 5;
pub(crate) const STRING: u8 = 6;
pub(crate) const BINARY_BLOB: u8 = 7;
pub(crate) const ARRAY: u8 = 8;
pub(crate) const OBJECT: u8 = 9;
pub(crate) const ARRAY_TYPED: u8 = 10;
pub(crate) const INT32: u8 = 11;
pub(crate) const UINT32: u8 = 12;
pub(crate) const BOOLEAN_TRUE: u8 = 13;
pub(crate) const BOOLEAN_FALSE: u8 = 14;
pub(crate) const INT64_ZERO: u8 = 15;
pub(crate) const INT64_ONE: u8 = 16;
pub(crate) const DOUBLE_ZERO: u8 = 17;
pub(crate) const DOUBLE_ONE: u8 = 18;
pub(crate) const FLOAT: u8 = 19;
pub(crate) const INT16: u8 = 20;
pub(crate) const UINT16: u8 = 21;
pub(crate) const INT32_AS_BYTE: u8 = 23;
pub(crate) const ARRAY_TYPE_BYTE_LENGTH: u8 = 24;
pub(crate) const ARRAY_TYPE_AUXILIARY_BUFFER: u8 = 25;

/// The type code a scalar kind is stored under unless told otherwise.
///
/// Integers take the 4-byte type when they fit and the 8-byte one when not. Doubles are never
/// narrowed. With `shorthand`, `0` and `1` (and `0.0`, `1.0`) are type codes alone.
pub(crate) fn default_scalar_type(kind: &Kind, shorthand: bool) -> Option<u8> {
    Some(match kind {
        Kind::Null => NULL,
        Kind::Bool(true) => BOOLEAN_TRUE,
        Kind::Bool(false) => BOOLEAN_FALSE,
        Kind::Int(0) if shorthand => INT64_ZERO,
        Kind::Int(1) if shorthand => INT64_ONE,
        Kind::Int(v) if i32::try_from(*v).is_ok() => INT32,
        Kind::Int(_) => INT64,
        Kind::UInt(v) if u32::try_from(*v).is_ok() => UINT32,
        Kind::UInt(_) => UINT64,
        Kind::Double(v) if shorthand && v.to_bits() == 0 => DOUBLE_ZERO,
        Kind::Double(v) if shorthand && v.to_bits() == 1.0f64.to_bits() => DOUBLE_ONE,
        Kind::Double(_) => DOUBLE,
        _ => return None,
    })
}

/// The operand a scalar kind has under type code `ty`, or `None` if that type cannot hold it
/// exactly. Types without an operand give `0`.
pub(crate) fn scalar_bits(kind: &Kind, ty: u8) -> Option<u64> {
    match (kind, ty) {
        (Kind::Null, NULL)
        | (Kind::Bool(true), BOOLEAN_TRUE)
        | (Kind::Bool(false), BOOLEAN_FALSE)
        | (Kind::Int(0), INT64_ZERO)
        | (Kind::Int(1), INT64_ONE) => Some(0),
        (Kind::Bool(b), BOOLEAN) => Some(u64::from(*b)),
        (Kind::Int(v), INT32_AS_BYTE) => i8::try_from(*v).ok().map(|v| u64::from(v as u8)),
        (Kind::Int(v), INT16) => i16::try_from(*v).ok().map(|v| u64::from(v as u16)),
        (Kind::Int(v), INT32) => i32::try_from(*v).ok().map(|v| u64::from(v as u32)),
        (Kind::Int(v), INT64) => Some(*v as u64),
        (Kind::UInt(v), UINT16) => u16::try_from(*v).ok().map(u64::from),
        (Kind::UInt(v), UINT32) => u32::try_from(*v).ok().map(u64::from),
        (Kind::UInt(v), UINT64) => Some(*v),
        (Kind::Double(v), DOUBLE_ZERO) if v.to_bits() == 0 => Some(0),
        (Kind::Double(v), DOUBLE_ONE) if v.to_bits() == 1.0f64.to_bits() => Some(0),
        (Kind::Double(v), FLOAT) => {
            let narrow = *v as f32;
            let lossless =
                f64::from(narrow).to_bits() == v.to_bits() || (v.is_nan() && narrow.is_nan());
            lossless.then(|| u64::from(narrow.to_bits()))
        }
        (Kind::Double(v), DOUBLE) => Some(v.to_bits()),
        _ => None,
    }
}

/// How many bytes a scalar type code reads from its pool, or `None` for a code that is not a
/// scalar.
pub(crate) fn operand_width(ty: u8) -> Option<usize> {
    match ty {
        NULL | BOOLEAN_TRUE | BOOLEAN_FALSE | INT64_ZERO | INT64_ONE | DOUBLE_ZERO | DOUBLE_ONE => {
            Some(0)
        }
        BOOLEAN | INT32_AS_BYTE => Some(1),
        INT16 | UINT16 => Some(2),
        INT32 | UINT32 | FLOAT => Some(4),
        INT64 | UINT64 | DOUBLE => Some(8),
        _ => None,
    }
}

/// The value a scalar type code and operand stand for.
pub(crate) fn scalar_value(ty: u8, bits: u64, flags: u8) -> Value {
    let kind = match ty {
        BOOLEAN_TRUE => Kind::Bool(true),
        BOOLEAN_FALSE => Kind::Bool(false),
        BOOLEAN => Kind::Bool(bits != 0),
        INT64_ZERO => Kind::Int(0),
        INT64_ONE => Kind::Int(1),
        DOUBLE_ZERO => Kind::Double(0.0),
        DOUBLE_ONE => Kind::Double(1.0),
        INT32_AS_BYTE => Kind::Int(i64::from(bits as u8 as i8)),
        INT16 => Kind::Int(i64::from(bits as u16 as i16)),
        UINT16 | UINT32 | UINT64 => Kind::UInt(bits),
        INT32 => Kind::Int(i64::from(bits as u32 as i32)),
        FLOAT => Kind::Double(f64::from(f32::from_bits(bits as u32))),
        INT64 => Kind::Int(bits as i64),
        DOUBLE => Kind::Double(f64::from_bits(bits)),
        _ => Kind::Null,
    };
    Value::stored(kind, flags, Storage::Scalar(ty))
}
