//! Cycle-level benchmarks for FIX delimiter scanning.
//!
//! Tests `find_soh` and `find_eq` at various buffer lengths to show
//! the SIMD dispatch cost and cycles/byte throughput.
//!
//! Run with:
//! ```bash
//! # SSE2 (default on x86_64)
//! cargo build --release --example perf_scan -p nexus-fix-codec
//! taskset -c 0 ./target/release/examples/perf_scan
//!
//! # AVX2
//! RUSTFLAGS="-C target-feature=+avx2" cargo build --release --example perf_scan -p nexus-fix-codec
//! taskset -c 0 ./target/release/examples/perf_scan
//! ```

#[path = "_bench_utils.rs"]
mod _bench_utils;

use _bench_utils::{ITERATIONS, WARMUP, percentile, print_intro, rdtsc};
use nexus_fix_codec::reader::FieldReader;
use nexus_fix_codec::scan;
use std::hint::black_box;

fn benchmark<T, F: FnMut() -> T>(mut f: F) -> (u64, u64, u64) {
    for _ in 0..WARMUP {
        black_box(f());
    }

    let mut samples = Vec::with_capacity(ITERATIONS);
    for _ in 0..ITERATIONS {
        let start = rdtsc();
        black_box(f());
        let end = rdtsc();
        samples.push(end.wrapping_sub(start));
    }

    samples.sort_unstable();
    (
        percentile(&samples, 50.0),
        percentile(&samples, 99.0),
        percentile(&samples, 99.9),
    )
}

fn main() {
    print_intro("FIX SCAN CYCLE BENCHMARK");

    #[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
    println!("SIMD: AVX2 (32 bytes/iteration)\n");
    #[cfg(all(target_arch = "x86_64", not(target_feature = "avx2")))]
    println!("SIMD: SSE2 (16 bytes/iteration)\n");
    #[cfg(not(target_arch = "x86_64"))]
    println!("SIMD: Scalar SWAR (8 bytes/iteration)\n");

    // =========================================================================
    // find_soh: target at end (worst case — full scan)
    // =========================================================================

    let lengths = [
        4, 8, 12, 16, 20, 24, 32, 48, 64, 96, 128, 192, 256, 384, 512, 768, 1024,
    ];

    println!("=== find_soh: target at end (worst case full scan) ===\n");
    println!(
        "{:<30} {:>8} {:>8} {:>8} {:>10}",
        "Length", "p50", "p99", "p999", "cyc/byte"
    );
    println!("{}", "-".repeat(70));

    for &len in &lengths {
        let mut buf = vec![b'A'; len];
        *buf.last_mut().unwrap() = 0x01;

        let (p50, p99, p999) = benchmark(|| scan::find_soh(black_box(&buf), 0));

        println!(
            "{:<30} {:>8} {:>8} {:>8} {:>10.2}",
            format!("{}B", len),
            p50,
            p99,
            p999,
            p50 as f64 / len as f64
        );
    }

    // =========================================================================
    // find_soh: no match (scan entire buffer, return None)
    // =========================================================================

    println!("\n=== find_soh: no match (full scan, return None) ===\n");
    println!(
        "{:<30} {:>8} {:>8} {:>8} {:>10}",
        "Length", "p50", "p99", "p999", "cyc/byte"
    );
    println!("{}", "-".repeat(70));

    for &len in &lengths {
        let buf = vec![b'A'; len];

        let (p50, p99, p999) = benchmark(|| scan::find_soh(black_box(&buf), 0));

        println!(
            "{:<30} {:>8} {:>8} {:>8} {:>10.2}",
            format!("{}B", len),
            p50,
            p99,
            p999,
            p50 as f64 / len as f64
        );
    }

    // =========================================================================
    // find_eq: tag=value separation (typical short scan)
    // =========================================================================

    println!("\n=== find_eq: typical tag=value (target near start) ===\n");
    println!("{:<30} {:>8} {:>8} {:>8}", "Scenario", "p50", "p99", "p999");
    println!("{}", "-".repeat(58));

    // 1-digit tag: "8=..."
    let field_1d = b"8=FIX.4.4\x01";
    let (p50, p99, p999) = benchmark(|| scan::find_eq(black_box(field_1d.as_slice()), 0));
    println!(
        "{:<30} {:>8} {:>8} {:>8}",
        "1-digit tag (8=)", p50, p99, p999
    );

    // 2-digit tag: "35=..."
    let field_2d = b"35=D\x01";
    let (p50, p99, p999) = benchmark(|| scan::find_eq(black_box(field_2d.as_slice()), 0));
    println!(
        "{:<30} {:>8} {:>8} {:>8}",
        "2-digit tag (35=)", p50, p99, p999
    );

    // 3-digit tag: "150=..."
    let field_3d = b"150=2\x01";
    let (p50, p99, p999) = benchmark(|| scan::find_eq(black_box(field_3d.as_slice()), 0));
    println!(
        "{:<30} {:>8} {:>8} {:>8}",
        "3-digit tag (150=)", p50, p99, p999
    );

    // 4-digit tag: "5592=..."
    let field_4d = b"5592=CUSTOM\x01";
    let (p50, p99, p999) = benchmark(|| scan::find_eq(black_box(field_4d.as_slice()), 0));
    println!(
        "{:<30} {:>8} {:>8} {:>8}",
        "4-digit tag (5592=)", p50, p99, p999
    );

    // =========================================================================
    // Realistic: scan all SOH delimiters in a FIX NewOrderSingle
    // =========================================================================

    println!("\n=== Realistic: scan all SOH in NewOrderSingle ===\n");

    let msg = b"8=FIX.4.4\x019=120\x0135=D\x0149=SENDER\x0156=TARGET\x01\
                34=42\x0152=20260530-12:00:00.000\x0111=order-001\x01\
                55=BTC-USD\x0154=1\x0138=1.50000000\x0140=2\x01\
                44=67500.00\x0159=0\x0110=178\x01";

    let msg_len = msg.len();
    let (p50, p99, p999) = benchmark(|| {
        let buf = black_box(msg.as_slice());
        let mut pos = 0;
        let mut count = 0u64;
        while let Some(soh) = scan::find_soh(buf, pos) {
            count += 1;
            pos = soh + 1;
        }
        count
    });

    println!("  Message length: {} bytes, 15 fields", msg_len);

    println!("\n  find_soh loop (re-scan per call):");
    println!("    p50={} p99={} p999={} cycles", p50, p99, p999);
    println!(
        "    {:.1} cycles/field, {:.2} cycles/byte",
        p50 as f64 / 15.0,
        p50 as f64 / msg_len as f64
    );

    let (p50_iter, p99_iter, p999_iter) = benchmark(|| {
        let buf = black_box(msg.as_slice());
        scan::soh_iter(buf, 0).count() as u64
    });

    println!("\n  soh_iter (mask-cached):");
    println!(
        "    p50={} p99={} p999={} cycles",
        p50_iter, p99_iter, p999_iter
    );
    println!(
        "    {:.1} cycles/field, {:.2} cycles/byte",
        p50_iter as f64 / 15.0,
        p50_iter as f64 / msg_len as f64
    );

    // =========================================================================
    // FieldReader: fused scan + tag parse + checksum
    // =========================================================================

    println!("\n=== FieldReader: fused scan + tag + checksum ===\n");

    let (p50_parse, p99_parse, p999_parse) = benchmark(|| {
        let buf = black_box(msg.as_slice());
        let mut parser = FieldReader::new(buf, 0);
        let mut count = 0u64;
        while parser.next_field().is_some() {
            count += 1;
        }
        black_box(parser.checksum());
        count
    });

    println!("  FieldReader (scan + tag parse + checksum):");
    println!(
        "    p50={} p99={} p999={} cycles",
        p50_parse, p99_parse, p999_parse
    );
    println!(
        "    {:.1} cycles/field, {:.2} cycles/byte",
        p50_parse as f64 / 15.0,
        p50_parse as f64 / msg_len as f64
    );

    println!(
        "\n  Overhead vs soh_iter: {} cycles ({:.1}%)",
        p50_parse.saturating_sub(p50_iter),
        if p50_iter > 0 {
            (p50_parse.saturating_sub(p50_iter)) as f64 / p50_iter as f64 * 100.0
        } else {
            0.0
        }
    );
}
