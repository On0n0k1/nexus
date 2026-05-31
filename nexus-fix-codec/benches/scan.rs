use criterion::{Criterion, black_box, criterion_group, criterion_main};
use nexus_fix_codec::scan;

fn bench_find_soh(c: &mut Criterion) {
    let mut group = c.benchmark_group("find_soh");

    for &len in &[32, 64, 128, 256, 512, 1024] {
        let mut buf = vec![b'A'; len];
        *buf.last_mut().unwrap() = 0x01;

        group.bench_function(format!("{}B_end", len), |b| {
            b.iter(|| scan::find_soh(black_box(&buf), 0));
        });
    }

    group.finish();
}

fn bench_find_eq(c: &mut Criterion) {
    let mut group = c.benchmark_group("find_eq");

    for &len in &[32, 64, 128, 256, 512, 1024] {
        let mut buf = vec![b'A'; len];
        *buf.last_mut().unwrap() = b'=';

        group.bench_function(format!("{}B_end", len), |b| {
            b.iter(|| scan::find_eq(black_box(&buf), 0));
        });
    }

    group.finish();
}

fn bench_realistic_message(c: &mut Criterion) {
    let msg = b"8=FIX.4.4\x019=120\x0135=D\x0149=SENDER\x0156=TARGET\x01\
                34=42\x0152=20260530-12:00:00.000\x0111=order-001\x01\
                55=BTC-USD\x0154=1\x0138=1.50000000\x0140=2\x01\
                44=67500.00\x0159=0\x0110=178\x01";

    c.bench_function("scan_all_soh_neworder", |b| {
        b.iter(|| {
            let buf = black_box(msg.as_slice());
            let mut pos = 0;
            let mut count = 0u32;
            while let Some(soh) = scan::find_soh(buf, pos) {
                count += 1;
                pos = soh + 1;
            }
            count
        });
    });
}

criterion_group!(
    benches,
    bench_find_soh,
    bench_find_eq,
    bench_realistic_message
);
criterion_main!(benches);
