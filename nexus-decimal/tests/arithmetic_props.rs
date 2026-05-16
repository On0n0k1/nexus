//! Property-based tests for core arithmetic invariants.
//!
//! - Additive and multiplicative identity
//! - Add/sub roundtrip
//! - Commutativity of checked_add and checked_mul
//! - Truncation direction for div_pow2
//! - Cross-backing equivalence (i32 vs i64 at same D)

use nexus_decimal::Decimal;
use proptest::prelude::*;

type D32 = Decimal<i32, 4>;
type D64 = Decimal<i64, 8>;
type D128 = Decimal<i128, 18>;
type D64_4 = Decimal<i64, 4>;

proptest! {
    // ------- additive identity -------

    #[test]
    fn additive_identity_i32(raw: i32) {
        let a = D32::from_raw(raw);
        prop_assert_eq!(a + D32::ZERO, a);
    }

    #[test]
    fn additive_identity_i64(raw: i64) {
        let a = D64::from_raw(raw);
        prop_assert_eq!(a + D64::ZERO, a);
    }

    #[test]
    fn additive_identity_i128(raw: i128) {
        let a = D128::from_raw(raw);
        prop_assert_eq!(a + D128::ZERO, a);
    }

    // ------- multiplicative identity -------

    #[test]
    fn multiplicative_identity_i32(raw: i32) {
        let a = D32::from_raw(raw);
        if let Some(result) = a.checked_mul(D32::ONE) {
            prop_assert_eq!(result, a);
        }
    }

    #[test]
    fn multiplicative_identity_i64(raw: i64) {
        let a = D64::from_raw(raw);
        if let Some(result) = a.checked_mul(D64::ONE) {
            prop_assert_eq!(result, a);
        }
    }

    #[test]
    fn multiplicative_identity_i128(raw: i128) {
        let a = D128::from_raw(raw);
        if let Some(result) = a.checked_mul(D128::ONE) {
            prop_assert_eq!(result, a);
        }
    }

    // ------- add/sub roundtrip -------

    #[test]
    fn add_sub_roundtrip_i32(a: i32, b: i32) {
        let a = D32::from_raw(a);
        let b = D32::from_raw(b);
        if let Some(sum) = a.checked_add(b) {
            prop_assert_eq!(sum.checked_sub(b), Some(a));
        }
    }

    #[test]
    fn add_sub_roundtrip_i64(a: i64, b: i64) {
        let a = D64::from_raw(a);
        let b = D64::from_raw(b);
        if let Some(sum) = a.checked_add(b) {
            prop_assert_eq!(sum.checked_sub(b), Some(a));
        }
    }

    #[test]
    fn add_sub_roundtrip_i128(a: i128, b: i128) {
        let a = D128::from_raw(a);
        let b = D128::from_raw(b);
        if let Some(sum) = a.checked_add(b) {
            prop_assert_eq!(sum.checked_sub(b), Some(a));
        }
    }

    // ------- commutativity of checked_add -------

    #[test]
    fn checked_add_commutative_i32(a: i32, b: i32) {
        let a = D32::from_raw(a);
        let b = D32::from_raw(b);
        prop_assert_eq!(a.checked_add(b), b.checked_add(a));
    }

    #[test]
    fn checked_add_commutative_i64(a: i64, b: i64) {
        let a = D64::from_raw(a);
        let b = D64::from_raw(b);
        prop_assert_eq!(a.checked_add(b), b.checked_add(a));
    }

    #[test]
    fn checked_add_commutative_i128(a: i128, b: i128) {
        let a = D128::from_raw(a);
        let b = D128::from_raw(b);
        prop_assert_eq!(a.checked_add(b), b.checked_add(a));
    }

    // ------- commutativity of checked_mul -------

    #[test]
    fn checked_mul_commutative_i32(a: i32, b: i32) {
        let a = D32::from_raw(a);
        let b = D32::from_raw(b);
        prop_assert_eq!(a.checked_mul(b), b.checked_mul(a));
    }

    #[test]
    fn checked_mul_commutative_i64(a: i64, b: i64) {
        let a = D64::from_raw(a);
        let b = D64::from_raw(b);
        prop_assert_eq!(a.checked_mul(b), b.checked_mul(a));
    }

    #[test]
    fn checked_mul_commutative_i128(a: i128, b: i128) {
        let a = D128::from_raw(a);
        let b = D128::from_raw(b);
        prop_assert_eq!(a.checked_mul(b), b.checked_mul(a));
    }

    // ------- truncation direction for div_pow2 -------

    #[test]
    fn div_pow2_truncates_toward_zero_i32(raw: i32, n in 1u32..i32::BITS) {
        let v = D32::from_raw(raw);
        let divided = v.div_pow2(n);
        if let Some(back) = divided.checked_mul_pow2(n) {
            if raw >= 0 {
                prop_assert!(back.to_raw() <= raw, "positive: div_pow2 should truncate down");
            } else {
                prop_assert!(back.to_raw() >= raw, "negative: div_pow2 should truncate toward zero");
            }
        }
    }

    #[test]
    fn div_pow2_truncates_toward_zero_i64(raw: i64, n in 1u32..i64::BITS) {
        let v = D64::from_raw(raw);
        let divided = v.div_pow2(n);
        if let Some(back) = divided.checked_mul_pow2(n) {
            if raw >= 0 {
                prop_assert!(back.to_raw() <= raw, "positive: div_pow2 should truncate down");
            } else {
                prop_assert!(back.to_raw() >= raw, "negative: div_pow2 should truncate toward zero");
            }
        }
    }

    #[test]
    fn div_pow2_truncates_toward_zero_i128(raw: i128, n in 1u32..i128::BITS) {
        let v = D128::from_raw(raw);
        let divided = v.div_pow2(n);
        if let Some(back) = divided.checked_mul_pow2(n) {
            if raw >= 0 {
                prop_assert!(back.to_raw() <= raw, "positive: div_pow2 should truncate down");
            } else {
                prop_assert!(back.to_raw() >= raw, "negative: div_pow2 should truncate toward zero");
            }
        }
    }

    // ------- cross-backing equivalence (i32 vs i64 at D=4) -------

    #[test]
    fn cross_backing_add(a: i32, b: i32) {
        let a32 = D32::from_raw(a);
        let b32 = D32::from_raw(b);
        let a64 = D64_4::from_raw(a as i64);
        let b64 = D64_4::from_raw(b as i64);

        if let Some(sum32) = a32.checked_add(b32) {
            let sum64 = a64.checked_add(b64).unwrap();
            prop_assert_eq!(sum32.to_raw() as i64, sum64.to_raw());
        }
    }

    #[test]
    fn cross_backing_sub(a: i32, b: i32) {
        let a32 = D32::from_raw(a);
        let b32 = D32::from_raw(b);
        let a64 = D64_4::from_raw(a as i64);
        let b64 = D64_4::from_raw(b as i64);

        if let Some(diff32) = a32.checked_sub(b32) {
            let diff64 = a64.checked_sub(b64).unwrap();
            prop_assert_eq!(diff32.to_raw() as i64, diff64.to_raw());
        }
    }

    #[test]
    fn cross_backing_mul(a: i32, b: i32) {
        let a32 = D32::from_raw(a);
        let b32 = D32::from_raw(b);
        let a64 = D64_4::from_raw(a as i64);
        let b64 = D64_4::from_raw(b as i64);

        if let Some(prod32) = a32.checked_mul(b32) {
            let prod64 = a64.checked_mul(b64).unwrap();
            prop_assert_eq!(prod32.to_raw() as i64, prod64.to_raw());
        }
    }

    #[test]
    fn cross_backing_div_pow2(raw: i32, n in 0u32..i32::BITS) {
        let v32 = D32::from_raw(raw);
        let v64 = D64_4::from_raw(raw as i64);
        prop_assert_eq!(v32.div_pow2(n).to_raw() as i64, v64.div_pow2(n).to_raw());
    }
}
