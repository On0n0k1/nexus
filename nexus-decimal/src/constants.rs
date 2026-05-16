//! Named constants for common decimal values.
//!
//! Core constants (`ZERO`, `ONE`, `MAX`, `MIN`) are generated for each
//! backing type. Financial constants (CENT, BASIS_POINT, etc.) are
//! in `financial.rs`.

use crate::Decimal;

macro_rules! impl_decimal_constants {
    ($backing:ty) => {
        impl<const D: u8> Decimal<$backing, D> {
            /// Zero (`0`).
            pub const ZERO: Self = Self { value: 0 };

            /// One (`1.0`).
            pub const ONE: Self = Self { value: Self::SCALE };

            /// Negative one (`-1.0`).
            pub const NEG_ONE: Self = Self {
                value: -Self::SCALE,
            };

            /// Maximum representable value.
            pub const MAX: Self = Self {
                value: <$backing>::MAX,
            };

            /// Minimum representable value.
            pub const MIN: Self = Self {
                value: <$backing>::MIN,
            };

            /// Smallest positive representable value (`from_raw(1)`).
            ///
            /// Represents `1 / 10^D` — the resolution of this decimal type.
            pub const EPSILON: Self = Self { value: 1 };

            /// One half (`0.5`).
            ///
            /// # Compile-time constraint
            ///
            /// Requires `D >= 1`. Instantiating `HALF` on a `Decimal` with
            /// `D = 0` is a compile error — the value 0.5 is not
            /// representable with zero fractional digits.
            pub const HALF: Self = {
                assert!(
                    D >= 1,
                    "HALF requires D >= 1: 0.5 is not representable with zero fractional digits"
                );
                Self {
                    value: Self::SCALE / 2,
                }
            };

            /// Two (`2.0`).
            ///
            /// # Compile-time constraint
            ///
            /// Requires the backing type to be wide enough that `2 * SCALE`
            /// does not overflow. This holds for all valid `D` on `i32` and
            /// `i64`; on `i128` it fails at `D = 38`.
            pub const TWO: Self = {
                assert!(
                    Self::SCALE <= <$backing>::MAX / 2,
                    "TWO requires 2*SCALE to fit in the backing type"
                );
                Self {
                    value: Self::SCALE * 2,
                }
            };
        }
    };
}

impl_decimal_constants!(i32);
impl_decimal_constants!(i64);
impl_decimal_constants!(i128);
