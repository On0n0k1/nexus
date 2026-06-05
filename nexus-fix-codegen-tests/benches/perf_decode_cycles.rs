use std::hint::black_box;

use nexus_fix_codegen_tests::{venue_alpha, venue_beta};

const WARMUP: usize = 5_000;
const SAMPLES: usize = 20_000;
const BATCH: u64 = 100;

#[inline(always)]
fn rdtsc_fenced_start() -> u64 {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::x86_64::_mm_lfence();
        core::arch::x86_64::_rdtsc()
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        std::time::Instant::now().elapsed().as_nanos() as u64
    }
}

#[inline(always)]
fn rdtsc_fenced_end() -> u64 {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        let mut aux = 0u32;
        let tsc = core::arch::x86_64::__rdtscp(&raw mut aux);
        core::arch::x86_64::_mm_lfence();
        tsc
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        std::time::Instant::now().elapsed().as_nanos() as u64
    }
}

fn measure<F: Fn() -> R, R>(name: &str, f: F) {
    for _ in 0..WARMUP {
        black_box(f());
    }

    let mut samples = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let start = rdtsc_fenced_start();
        for _ in 0..BATCH {
            black_box(f());
        }
        let end = rdtsc_fenced_end();
        samples.push(end.wrapping_sub(start) / BATCH);
    }

    samples.sort_unstable();
    let p50 = samples[samples.len() / 2];
    let p99 = samples[(samples.len() as f64 * 0.99) as usize];
    let p999 = samples[(samples.len() as f64 * 0.999) as usize];
    let max = *samples.last().unwrap();

    println!(
        "{:<55} p50={:<5} p99={:<5} p99.9={:<5} max={:<6}",
        name, p50, p99, p999, max,
    );
}

fn build_alpha_nos_with_groups() -> Vec<u8> {
    let mut buf = [0u8; 512];
    let ts = nexus_fix_codec::FixTimestamp::parse(b"20260603-12:00:00").unwrap();
    let msg = venue_alpha::encoders::NewOrderSingleEncoder::wrap(&mut buf)
        .header_encoder()
        .sender_comp_id(b"SENDER")
        .target_comp_id(b"TARGET")
        .msg_seq_num(42)
        .sending_time(ts)
        .finish()
        .cl_ord_id(b"ORD-12345")
        .side(venue_alpha::fields::Side::BUY)
        .no_party_i_ds(2)
        .entry()
        .party_id(b"PARTY1")
        .party_role(1)
        .done()
        .entry()
        .party_id(b"PARTY2")
        .party_role(2)
        .done()
        .finish_group()
        .symbol(b"BTC-USD")
        .finish()
        .unwrap();
    msg.to_vec()
}

fn build_alpha_nos_no_groups() -> Vec<u8> {
    let mut buf = [0u8; 256];
    let ts = nexus_fix_codec::FixTimestamp::parse(b"20260603-12:00:00").unwrap();
    let msg = venue_alpha::encoders::NewOrderSingleEncoder::wrap(&mut buf)
        .header_encoder()
        .sender_comp_id(b"SENDER")
        .target_comp_id(b"TARGET")
        .msg_seq_num(42)
        .sending_time(ts)
        .finish()
        .cl_ord_id(b"ORD-12345")
        .side(venue_alpha::fields::Side::BUY)
        .symbol(b"BTC-USD")
        .finish()
        .unwrap();
    msg.to_vec()
}

fn build_beta_md_with_groups() -> Vec<u8> {
    let mut buf = [0u8; 512];
    let px_bid = nexus_fix_codec::FixDecimal { mantissa: 11050, scale: 4 };
    let px_offer = nexus_fix_codec::FixDecimal { mantissa: 11052, scale: 4 };
    let sz = nexus_fix_codec::FixDecimal { mantissa: 1_000_000, scale: 0 };
    let msg = venue_beta::encoders::MarketDataSnapshotFullRefreshEncoder::wrap(&mut buf)
        .header_encoder()
        .finish()
        .symbol(b"EUR/USD")
        .no_md_entries(2)
        .entry()
        .md_entry_type(venue_beta::fields::MDEntryType::BID)
        .md_entry_px(px_bid)
        .md_entry_size(sz)
        .done()
        .entry()
        .md_entry_type(venue_beta::fields::MDEntryType::OFFER)
        .md_entry_px(px_offer)
        .md_entry_size(sz)
        .done()
        .finish_group()
        .finish()
        .unwrap();
    msg.to_vec()
}

fn main() {
    println!("FIX decode cycle measurements ({SAMPLES} samples, {WARMUP} warmup, batch {BATCH})\n");
    println!(
        "{:<55} {:>5}  {:>5}  {:>5}  {:>6}",
        "benchmark", "p50", "p99", "p99.9", "max"
    );
    println!("{}", "-".repeat(90));

    // -- Header only --

    let header_msg = b"8=FIX.4.4\x019=120\x0135=D\x0149=SENDER\x0156=TARGET\x0134=42\x0152=20260603-12:00:00\x01";
    measure("header_decode  (7 fields)", || {
        venue_alpha::header::HeaderDecoder::decode(black_box(header_msg))
    });

    // -- NOS without groups --

    let nos_no_grp = build_alpha_nos_no_groups();
    measure("alpha NOS decode  (no groups)", || {
        venue_alpha::messages::NewOrderSingle::decode(black_box(&nos_no_grp))
    });

    // -- NOS with 2-entry party group --

    let nos_grp = build_alpha_nos_with_groups();
    measure("alpha NOS decode  (2 parties)", || {
        venue_alpha::messages::NewOrderSingle::decode(black_box(&nos_grp))
    });

    // -- NOS decode + iterate group entries --

    measure("alpha NOS decode+iterate  (2 parties)", || {
        let m = venue_alpha::messages::NewOrderSingle::decode(black_box(&nos_grp)).unwrap();
        let mut count = 0u32;
        for p in m.no_party_i_ds() {
            black_box(p.party_id());
            count += 1;
        }
        count
    });

    // -- Beta MD snapshot with 2-entry group --

    let md_grp = build_beta_md_with_groups();
    measure("beta MD decode  (2 entries)", || {
        venue_beta::messages::MarketDataSnapshotFullRefresh::decode(black_box(&md_grp))
    });

    measure("beta MD decode+iterate  (2 entries)", || {
        let m = venue_beta::messages::MarketDataSnapshotFullRefresh::decode(black_box(&md_grp)).unwrap();
        let mut count = 0u32;
        for e in m.no_md_entries() {
            black_box(e.md_entry_px());
            count += 1;
        }
        count
    });

    println!();

    // -- Encode benchmarks --

    println!("=== ENCODE ===\n");

    let ts = nexus_fix_codec::FixTimestamp::parse(b"20260603-12:00:00").unwrap();

    measure("alpha NOS encode  (no groups)", || {
        let mut buf = [0u8; 256];
        let msg = venue_alpha::encoders::NewOrderSingleEncoder::wrap(black_box(&mut buf))
            .header_encoder()
            .sender_comp_id(b"SENDER")
            .target_comp_id(b"TARGET")
            .msg_seq_num(42)
            .sending_time(ts)
            .finish()
            .cl_ord_id(b"ORD-12345")
            .side(venue_alpha::fields::Side::BUY)
            .symbol(b"BTC-USD")
            .finish()
            .unwrap();
        black_box(msg.len())
    });

    measure("alpha NOS encode  (2 parties)", || {
        let mut buf = [0u8; 512];
        let msg = venue_alpha::encoders::NewOrderSingleEncoder::wrap(black_box(&mut buf))
            .header_encoder()
            .sender_comp_id(b"SENDER")
            .target_comp_id(b"TARGET")
            .msg_seq_num(42)
            .sending_time(ts)
            .finish()
            .cl_ord_id(b"ORD-12345")
            .side(venue_alpha::fields::Side::BUY)
            .no_party_i_ds(2)
            .entry()
            .party_id(b"PARTY1")
            .party_role(1)
            .done()
            .entry()
            .party_id(b"PARTY2")
            .party_role(2)
            .done()
            .finish_group()
            .symbol(b"BTC-USD")
            .finish()
            .unwrap();
        black_box(msg.len())
    });
}
