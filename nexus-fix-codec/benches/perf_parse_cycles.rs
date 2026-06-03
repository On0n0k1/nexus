//! CPU cycle measurement for FIX type parsers.
//!
//! Build: cargo build --release --bench perf_parse_cycles
//! Run:   sudo taskset -c 0 ./target/release/deps/perf_parse_cycles-*

use std::hint::black_box;

const WARMUP: usize = 10_000;
const SAMPLES: usize = 100_000;

#[inline]
fn rdtscp() -> u64 {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        let mut aux: u32 = 0;
        core::arch::x86_64::__rdtscp(&raw mut aux)
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        use std::time::Instant;
        static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        START.get_or_init(Instant::now).elapsed().as_nanos() as u64
    }
}

fn measure<F: Fn() -> R, R>(name: &str, f: F) {
    let mut samples = Vec::with_capacity(SAMPLES);

    for i in 0..WARMUP + SAMPLES {
        let start = rdtscp();
        black_box(f());
        let elapsed = rdtscp() - start;

        if i >= WARMUP {
            samples.push(elapsed);
        }
    }

    samples.sort_unstable();
    let min = samples[0];
    let p50 = samples[samples.len() / 2];
    let p99 = samples[(samples.len() as f64 * 0.99) as usize];
    let p999 = samples[(samples.len() as f64 * 0.999) as usize];
    let max = *samples.last().unwrap();
    let mean = samples.iter().sum::<u64>() as f64 / samples.len() as f64;

    println!(
        "{:<40} min={:<4} p50={:<4} p99={:<5} p99.9={:<5} max={:<6} mean={:.1}",
        name, min, p50, p99, p999, max, mean
    );
}

fn main() {
    use nexus_fix_codec::*;

    println!("FIX type parser cycle measurements ({SAMPLES} samples, {WARMUP} warmup)\n");
    println!(
        "{:<40} {:>4}  {:>4}  {:>5}  {:>5}  {:>6}  {:>6}",
        "benchmark", "min", "p50", "p99", "p99.9", "max", "mean"
    );
    println!("{}", "-".repeat(90));

    // -- FixDecimal --

    measure("FixDecimal  4-digit  \"99.50\"", || {
        FixDecimal::parse(black_box(b"99.50"))
    });
    measure("FixDecimal  8-digit  \"50123.450\"", || {
        FixDecimal::parse(black_box(b"50123.450"))
    });
    measure("FixDecimal 12-digit  \"50123.45000000\"", || {
        FixDecimal::parse(black_box(b"50123.45000000"))
    });
    measure("FixDecimal 16-digit  \"1234567.890123456\"", || {
        FixDecimal::parse(black_box(b"1234567.890123456"))
    });
    measure("FixDecimal integer   \"12345678\"", || {
        FixDecimal::parse(black_box(b"12345678"))
    });
    measure("FixDecimal negative  \"-123.456\"", || {
        FixDecimal::parse(black_box(b"-123.456"))
    });
    measure("FixDecimal sub-penny \"0.00000001\"", || {
        FixDecimal::parse(black_box(b"0.00000001"))
    });

    println!();

    // -- parse_fix_int --

    measure("parse_fix_int  1-digit  \"7\"", || {
        parse_fix_int(black_box(b"7"))
    });
    measure("parse_fix_int  4-digit  \"1234\"", || {
        parse_fix_int(black_box(b"1234"))
    });
    measure("parse_fix_int  8-digit  \"12345678\"", || {
        parse_fix_int(black_box(b"12345678"))
    });
    measure("parse_fix_int 16-digit  \"1234567890123456\"", || {
        parse_fix_int(black_box(b"1234567890123456"))
    });
    measure("parse_fix_int 19-digit  i64::MAX", || {
        parse_fix_int(black_box(b"9223372036854775807"))
    });
    measure("parse_fix_int negative  \"-12345678\"", || {
        parse_fix_int(black_box(b"-12345678"))
    });

    println!();

    // -- parse_fix_uint / parse_fix_seqnum --

    measure("parse_fix_uint  \"256\"", || {
        parse_fix_uint(black_box(b"256"))
    });
    measure("parse_fix_seqnum  \"1000000\"", || {
        parse_fix_seqnum(black_box(b"1000000"))
    });
    measure("parse_fix_seqnum  \"99999999999\"", || {
        parse_fix_seqnum(black_box(b"99999999999"))
    });

    println!();

    // -- FixTimestamp --

    measure("FixTimestamp  no_frac", || {
        FixTimestamp::parse(black_box(b"20260602-14:30:00"))
    });
    measure("FixTimestamp  millis", || {
        FixTimestamp::parse(black_box(b"20260602-14:30:00.123"))
    });
    measure("FixTimestamp  micros", || {
        FixTimestamp::parse(black_box(b"20260602-14:30:00.123456"))
    });
    measure("FixTimestamp  nanos", || {
        FixTimestamp::parse(black_box(b"20260602-14:30:00.123456789"))
    });

    println!();

    // -- FixDate / FixTime --

    measure("FixDate  \"20260602\"", || {
        FixDate::parse(black_box(b"20260602"))
    });
    measure("FixTime  no_frac \"14:30:00\"", || {
        FixTime::parse(black_box(b"14:30:00"))
    });
    measure("FixTime  micros  \"14:30:00.123456\"", || {
        FixTime::parse(black_box(b"14:30:00.123456"))
    });

    println!();

    // -- parse_fix_bool --

    measure("parse_fix_bool  \"Y\"", || parse_fix_bool(black_box(b"Y")));
    measure("parse_fix_bool  \"N\"", || parse_fix_bool(black_box(b"N")));

    println!();
    println!("=== ENCODE ===");
    println!();

    // -- FixDecimal::encode --

    let dec_int = FixDecimal::parse(b"12345678").unwrap();
    let dec_4 = FixDecimal::parse(b"99.50").unwrap();
    let dec_8 = FixDecimal::parse(b"50123.450").unwrap();
    let dec_12 = FixDecimal::parse(b"50123.45000000").unwrap();
    let dec_16 = FixDecimal::parse(b"1234567.890123456").unwrap();
    let dec_neg = FixDecimal::parse(b"-123.456").unwrap();

    measure("FixDecimal::encode  integer  \"12345678\"", || {
        let mut buf = [0u8; 21];
        black_box(dec_int).encode(black_box(&mut buf))
    });
    measure("FixDecimal::encode  4-digit  \"99.50\"", || {
        let mut buf = [0u8; 21];
        black_box(dec_4).encode(black_box(&mut buf))
    });
    measure("FixDecimal::encode  8-digit  \"50123.450\"", || {
        let mut buf = [0u8; 21];
        black_box(dec_8).encode(black_box(&mut buf))
    });
    measure("FixDecimal::encode  12-digit \"50123.45000000\"", || {
        let mut buf = [0u8; 21];
        black_box(dec_12).encode(black_box(&mut buf))
    });
    measure("FixDecimal::encode  16-digit \"1234567.890123456\"", || {
        let mut buf = [0u8; 21];
        black_box(dec_16).encode(black_box(&mut buf))
    });
    measure("FixDecimal::encode  negative \"-123.456\"", || {
        let mut buf = [0u8; 21];
        black_box(dec_neg).encode(black_box(&mut buf))
    });

    println!();

    // -- encode_fix_int --

    measure("encode_fix_int  8-digit", || {
        let mut buf = [0u8; 20];
        encode_fix_int(black_box(12_345_678), black_box(&mut buf))
    });
    measure("encode_fix_int  16-digit", || {
        let mut buf = [0u8; 20];
        encode_fix_int(black_box(1_234_567_890_123_456), black_box(&mut buf))
    });
    measure("encode_fix_int  negative 8-digit", || {
        let mut buf = [0u8; 20];
        encode_fix_int(black_box(-12_345_678), black_box(&mut buf))
    });

    println!();

    // -- encode_fix_uint / encode_fix_seqnum --

    measure("encode_fix_uint  \"256\"", || {
        let mut buf = [0u8; 10];
        encode_fix_uint(black_box(256), black_box(&mut buf))
    });
    measure("encode_fix_seqnum  \"1000000\"", || {
        let mut buf = [0u8; 20];
        encode_fix_seqnum(black_box(1_000_000), black_box(&mut buf))
    });

    println!();

    // -- FixTimestamp::encode --

    let ts_no_frac = FixTimestamp::parse(b"20260602-14:30:00").unwrap();
    let ts_millis = FixTimestamp::parse(b"20260602-14:30:00.123").unwrap();
    let ts_micros = FixTimestamp::parse(b"20260602-14:30:00.123456").unwrap();
    let ts_nanos = FixTimestamp::parse(b"20260602-14:30:00.123456789").unwrap();

    measure("FixTimestamp::encode  no_frac", || {
        let mut buf = [0u8; 27];
        black_box(ts_no_frac).encode(black_box(&mut buf))
    });
    measure("FixTimestamp::encode  millis", || {
        let mut buf = [0u8; 27];
        black_box(ts_millis).encode(black_box(&mut buf))
    });
    measure("FixTimestamp::encode  micros", || {
        let mut buf = [0u8; 27];
        black_box(ts_micros).encode(black_box(&mut buf))
    });
    measure("FixTimestamp::encode  nanos", || {
        let mut buf = [0u8; 27];
        black_box(ts_nanos).encode(black_box(&mut buf))
    });

    println!();

    // -- FixDate / FixTime encode --

    let date = FixDate::parse(b"20260602").unwrap();
    let time_no_frac = FixTime::parse(b"14:30:00").unwrap();
    let time_micros = FixTime::parse(b"14:30:00.123456").unwrap();

    measure("FixDate::encode  \"20260602\"", || {
        let mut buf = [0u8; 8];
        black_box(date).encode(black_box(&mut buf))
    });
    measure("FixTime::encode  no_frac \"14:30:00\"", || {
        let mut buf = [0u8; 18];
        black_box(time_no_frac).encode(black_box(&mut buf))
    });
    measure("FixTime::encode  micros  \"14:30:00.123456\"", || {
        let mut buf = [0u8; 18];
        black_box(time_micros).encode(black_box(&mut buf))
    });

    println!();

    // -- encode_fix_bool --

    measure("encode_fix_bool  true", || encode_fix_bool(black_box(true)));
}
