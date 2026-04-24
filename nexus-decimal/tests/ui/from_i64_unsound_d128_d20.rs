// i64 is sound on Decimal<i128, _> up to D=19 only. At D=20:
// i64::MAX × 10^20 ≈ 9.22e38 > i128::MAX ≈ 1.7e38 → unsound.
// `From<i64>` is not emitted at D=20; caller must use TryFrom.

use nexus_decimal::Decimal;

type BadD = Decimal<i128, 20>;

fn main() {
    let _: BadD = 5_i64.into();
}
