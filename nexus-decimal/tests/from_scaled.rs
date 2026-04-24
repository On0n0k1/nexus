//! Tests for `Decimal::from_scaled(value, scale)`.
//!
//! Constructs a decimal from `value * 10^-scale`. Returns `None` when
//! `scale > D` or when the scaled value overflows the backing.

use nexus_decimal::Decimal;

type D32 = Decimal<i32, 4>;
type D64 = Decimal<i64, 8>;
type D128 = Decimal<i128, 18>;

// ============================================================================
// Happy path — core behavior (D64)
// ============================================================================

#[test]
fn unit_at_full_scale() {
    let v = D64::from_scaled(1, 8).unwrap();
    assert_eq!(v, D64::from_str_exact("0.00000001").unwrap());
}

#[test]
fn unit_at_partial_scale() {
    let v = D64::from_scaled(1, 5).unwrap();
    assert_eq!(v, D64::from_str_exact("0.00001").unwrap());
}

#[test]
fn unit_at_scale_zero() {
    let v = D64::from_scaled(1, 0).unwrap();
    assert_eq!(v, D64::from_i32(1).unwrap());
}

#[test]
fn zero_at_any_scale() {
    for scale in 0..=8 {
        assert_eq!(
            D64::from_scaled(0, scale).unwrap(),
            D64::from_i32(0).unwrap()
        );
    }
}

#[test]
fn negative_value_preserves_sign() {
    let v = D64::from_scaled(-1, 5).unwrap();
    assert_eq!(v, D64::from_str_exact("-0.00001").unwrap());
}

#[test]
fn arbitrary_value_and_scale() {
    let v = D64::from_scaled(123, 3).unwrap();
    assert_eq!(v, D64::from_str_exact("0.123").unwrap());
}

// ============================================================================
// Scale boundaries
// ============================================================================

#[test]
fn scale_exceeds_d_returns_none() {
    assert!(D64::from_scaled(1, 9).is_none());
    assert!(D64::from_scaled(1, 10).is_none());
    assert!(D64::from_scaled(1, u8::MAX).is_none());
}

#[test]
fn scale_equals_d_is_smallest_unit() {
    let v = D64::from_scaled(1, 8).unwrap();
    assert_eq!(v.to_raw(), 1);
}

#[test]
fn scale_excess_does_not_panic() {
    // Should return None, not panic, for very large scale values.
    let _ = D64::from_scaled(1, u8::MAX);
    let _ = D64::from_scaled(i64::MAX, u8::MAX);
    let _ = D64::from_scaled(i64::MIN, u8::MAX);
}

// ============================================================================
// Overflow on large value
// ============================================================================

#[test]
fn max_value_at_scale_zero_overflows() {
    // At scale=0, multiplier = SCALE = 10^8. i64::MAX × 10^8 overflows i64.
    assert!(D64::from_scaled(i64::MAX, 0).is_none());
}

#[test]
fn min_value_at_scale_zero_overflows() {
    assert!(D64::from_scaled(i64::MIN, 0).is_none());
}

#[test]
fn i64_max_at_full_scale_ok() {
    // scale = D means multiplier = 1, so any i64 value fits.
    assert!(D64::from_scaled(i64::MAX, 8).is_some());
    assert!(D64::from_scaled(i64::MIN, 8).is_some());
    assert_eq!(D64::from_scaled(i64::MAX, 8).unwrap().to_raw(), i64::MAX);
    assert_eq!(D64::from_scaled(i64::MIN, 8).unwrap().to_raw(), i64::MIN);
}

#[test]
fn capacity_boundary_d64() {
    // At D=8 on i64, integer-unit capacity = floor(i64::MAX / 10^8) = 92_233_720_368
    let safe = 92_233_720_368_i64;
    assert!(D64::from_scaled(safe, 0).is_some());
    let overflow = 92_233_720_369_i64;
    assert!(D64::from_scaled(overflow, 0).is_none());
}

// ============================================================================
// D32 (i32 backing, D=4) coverage
// ============================================================================

#[test]
fn d32_unit_at_full_scale() {
    let v = D32::from_scaled(1, 4).unwrap();
    assert_eq!(v, D32::from_str_exact("0.0001").unwrap());
}

#[test]
fn d32_unit_at_scale_zero() {
    let v = D32::from_scaled(1, 0).unwrap();
    assert_eq!(v, D32::from_i32(1).unwrap());
}

#[test]
fn d32_negative_preserves_sign() {
    let v = D32::from_scaled(-1, 2).unwrap();
    assert_eq!(v, D32::from_str_exact("-0.01").unwrap());
}

#[test]
fn d32_scale_exceeds_d_returns_none() {
    assert!(D32::from_scaled(1, 5).is_none());
    assert!(D32::from_scaled(1, u8::MAX).is_none());
}

#[test]
fn d32_max_value_at_scale_zero_overflows() {
    // i32::MAX × 10^4 overflows i32.
    assert!(D32::from_scaled(i32::MAX, 0).is_none());
}

#[test]
fn d32_max_at_full_scale_ok() {
    assert!(D32::from_scaled(i32::MAX, 4).is_some());
    assert!(D32::from_scaled(i32::MIN, 4).is_some());
}

// ============================================================================
// D128 (i128 backing, D=18) coverage
// ============================================================================

#[test]
fn d128_unit_at_full_scale() {
    let v = D128::from_scaled(1, 18).unwrap();
    assert_eq!(v.to_raw(), 1);
}

#[test]
fn d128_unit_at_partial_scale() {
    let v = D128::from_scaled(1, 6).unwrap();
    assert_eq!(v, D128::from_str_exact("0.000001").unwrap());
}

#[test]
fn d128_zero_at_any_scale() {
    for scale in 0..=18 {
        assert_eq!(D128::from_scaled(0, scale).unwrap().to_raw(), 0);
    }
}

#[test]
fn d128_full_range_at_full_scale() {
    assert!(D128::from_scaled(i128::MAX, 18).is_some());
    assert!(D128::from_scaled(i128::MIN, 18).is_some());
    assert_eq!(
        D128::from_scaled(i128::MAX, 18).unwrap().to_raw(),
        i128::MAX
    );
}

#[test]
fn d128_max_at_scale_zero_overflows() {
    assert!(D128::from_scaled(i128::MAX, 0).is_none());
}

// ============================================================================
// Const evaluability
// ============================================================================

#[test]
fn const_evaluable() {
    const TICK: Option<D64> = D64::from_scaled(1, 5);
    assert!(TICK.is_some());
    assert_eq!(TICK.unwrap(), D64::from_str_exact("0.00001").unwrap());
}

#[test]
fn const_overflow_returns_none() {
    const OVERFLOW: Option<D64> = D64::from_scaled(i64::MAX, 0);
    assert!(OVERFLOW.is_none());
}
