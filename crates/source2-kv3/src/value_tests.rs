//! `Value` accessors: which conversions are exact and which are named lossy.

use crate::{Kind, Value};

#[test]
fn as_i64_is_exact_across_widths_and_signedness() {
    assert_eq!(Value::int(-5).as_i64(), Some(-5));
    assert_eq!(Value::uint(5).as_i64(), Some(5));
    assert_eq!(Value::uint(i64::MAX as u64).as_i64(), Some(i64::MAX));
    assert_eq!(Value::uint(i64::MAX as u64 + 1).as_i64(), None);
    assert_eq!(Value::uint(u64::MAX).as_i64(), None);
    assert_eq!(Value::double(3.0).as_i64(), None);
    assert_eq!(Value::from("3").as_i64(), None);
}

#[test]
fn as_u64_is_exact_across_widths_and_signedness() {
    assert_eq!(Value::uint(u64::MAX).as_u64(), Some(u64::MAX));
    assert_eq!(Value::int(5).as_u64(), Some(5));
    assert_eq!(Value::int(0).as_u64(), Some(0));
    assert_eq!(Value::int(-1).as_u64(), None);
    assert_eq!(Value::int(i64::MIN).as_u64(), None);
    assert_eq!(Value::double(3.0).as_u64(), None);
    assert_eq!(Value::null().as_u64(), None);
}

#[test]
fn as_u32_fits_or_is_none() {
    assert_eq!(Value::int(7).as_u32(), Some(7));
    assert_eq!(Value::uint(u64::from(u32::MAX)).as_u32(), Some(u32::MAX));
    assert_eq!(Value::uint(u64::from(u32::MAX) + 1).as_u32(), None);
    assert_eq!(Value::int(-1).as_u32(), None);
}

#[test]
fn as_f64_takes_doubles_and_only_the_integers_it_can_hold() {
    assert_eq!(Value::double(1.5).as_f64(), Some(1.5));
    assert_eq!(Value::int(3).as_f64(), Some(3.0));
    assert_eq!(Value::uint(3).as_f64(), Some(3.0));
    assert_eq!(Value::int(1 << 53).as_f64(), Some(9_007_199_254_740_992.0));
    assert_eq!(
        Value::int(-(1 << 53)).as_f64(),
        Some(-9_007_199_254_740_992.0)
    );
    assert_eq!(Value::int((1 << 53) + 1).as_f64(), None);
    assert_eq!(Value::int(i64::MAX).as_f64(), None);
    assert_eq!(Value::int(i64::MIN).as_f64(), None);
    assert_eq!(Value::uint(u64::MAX).as_f64(), None);
    assert_eq!(Value::from("1.5").as_f64(), None);
}

#[test]
fn to_f64_lossy_rounds_any_integer() {
    assert_eq!(Value::int(i64::MAX).to_f64_lossy(), Some(i64::MAX as f64));
    assert_eq!(Value::uint(u64::MAX).to_f64_lossy(), Some(u64::MAX as f64));
    assert_eq!(Value::double(0.25).to_f64_lossy(), Some(0.25));
    assert_eq!(Value::null().to_f64_lossy(), None);
}

#[test]
fn to_f32_lossy_narrows() {
    assert_eq!(Value::double(0.1).to_f32_lossy(), Some(0.1f32));
    assert_eq!(Value::int(2).to_f32_lossy(), Some(2.0));
    assert_eq!(Value::from("x").to_f32_lossy(), None);
}

#[test]
fn from_impls_pick_signed_or_unsigned_by_the_source_type() {
    assert_eq!(Value::from(5i8).kind(), &Kind::Int(5));
    assert_eq!(Value::from(5i16).kind(), &Kind::Int(5));
    assert_eq!(Value::from(5i32).kind(), &Kind::Int(5));
    assert_eq!(Value::from(5i64).kind(), &Kind::Int(5));
    assert_eq!(Value::from(5u8).kind(), &Kind::UInt(5));
    assert_eq!(Value::from(5u16).kind(), &Kind::UInt(5));
    assert_eq!(Value::from(5u32).kind(), &Kind::UInt(5));
    assert_eq!(Value::from(5u64).kind(), &Kind::UInt(5));
    assert_eq!(Value::from(true).kind(), &Kind::Bool(true));
    assert_eq!(Value::from(1.5f64).kind(), &Kind::Double(1.5));
    assert_eq!(Value::from(0.5f32).kind(), &Kind::Double(0.5));
    assert_eq!(Value::from("s").kind(), &Kind::String("s".into()));
    assert_eq!(Value::default(), Value::null());
}

#[test]
fn values_collect_into_an_array() {
    let v: Value = (1..=3).map(Value::int).collect();
    assert_eq!(v.as_array().map(<[Value]>::len), Some(3));
}

#[test]
fn mutable_accessors_reach_arrays_objects_and_members() {
    let mut v = Value::array(vec![Value::int(1)]);
    v.as_array_mut().unwrap().push(Value::int(2));
    assert_eq!(v.as_array().unwrap().len(), 2);
    assert!(v.as_object_mut().is_none());
    assert!(v.get_mut("k").is_none());

    let mut o = Value::from(crate::Object::from(vec![("k", Value::int(1))]));
    *o.get_mut("k").unwrap() = Value::int(9);
    assert_eq!(o.get("k").and_then(Value::as_i64), Some(9));
    assert!(o.as_array_mut().is_none());
}

#[test]
fn into_kind_hands_back_the_contents() {
    assert_eq!(Value::int(4).with_flags(1).into_kind(), Kind::Int(4));
}
