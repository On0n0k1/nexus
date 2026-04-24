// u32 is always unsound on Decimal<i32, _>: u32::MAX > i32::MAX even at D=0.
// `From<u32>` is not emitted, and u32 is not in the i64/u64 TryFrom list,
// so .into() and .try_into() both fail.

use nexus_decimal::Decimal;

type BadD = Decimal<i32, 0>;

fn main() {
    let _: BadD = 5_u32.into();
}
