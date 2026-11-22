use sha2::{Digest, Sha256};

struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

pub(crate) struct Rng {
    s: [u64; 4],
}

impl Rng {
    pub(crate) fn new(seed: u64) -> Self {
        let mut sm = SplitMix64 { state: seed };
        Rng {
            s: [sm.next(), sm.next(), sm.next(), sm.next()],
        }
    }

    pub(crate) fn next_u64(&mut self) -> u64 {
        let result = self.s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = self.s[1] << 17;
        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];
        self.s[2] ^= t;
        self.s[3] = self.s[3].rotate_left(45);
        result
    }

    pub(crate) fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    pub(crate) fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }

    pub(crate) fn range_i64(&mut self, lo: i64, hi: i64) -> i64 {
        lo + self.below((hi - lo + 1) as u64) as i64
    }

    pub(crate) fn chance(&mut self, p: f64) -> bool {
        self.next_f64() < p
    }

    pub(crate) fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len() as u64) as usize]
    }
}

pub(crate) fn time_ordered_id(tag: &str, seq: u64, unix_millis: i64) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"ledger-perf/v1/");
    hasher.update(tag.as_bytes());
    hasher.update(b"/");
    hasher.update(seq.to_le_bytes());
    let digest = hasher.finalize();

    let mut bytes = [0u8; 16];
    bytes[..6].copy_from_slice(&(unix_millis as u64).to_be_bytes()[2..8]);
    bytes[6..16].copy_from_slice(&digest[..10]);
    bytes[6] = (bytes[6] & 0x0F) | 0x70;
    bytes[8] = (bytes[8] & 0x3F) | 0x80;

    let hex = |slice: &[u8]| slice.iter().map(|b| format!("{b:02x}")).collect::<String>();
    format!(
        "{}-{}-{}-{}-{}",
        hex(&bytes[..4]),
        hex(&bytes[4..6]),
        hex(&bytes[6..8]),
        hex(&bytes[8..10]),
        hex(&bytes[10..16])
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_sequence() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..1000 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn different_seed_different_sequence() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(43);
        let diffs = (0..100)
            .filter(|_| {
                let (x, y) = (a.next_u64(), b.next_u64());
                x != y
            })
            .count();
        assert_eq!(diffs, 100);
    }

    #[test]
    fn next_f64_in_unit_interval() {
        let mut rng = Rng::new(7);
        for _ in 0..10_000 {
            let v = rng.next_f64();
            assert!((0.0..1.0).contains(&v));
        }
    }

    #[test]
    fn below_and_range_respect_bounds() {
        let mut rng = Rng::new(9);
        for _ in 0..1000 {
            let v = rng.below(10);
            assert!(v < 10);
            let r = rng.range_i64(-3, 3);
            assert!((-3..=3).contains(&r));
        }
    }

    #[test]
    fn time_ordered_id_is_valid_uuid_v7_shape_and_deterministic() {
        let a = time_ordered_id("transactions", 1, 1_700_000_000_000);
        let b = time_ordered_id("transactions", 1, 1_700_000_000_000);
        assert_eq!(a, b);
        assert_eq!(a.len(), 36);
        let parts: Vec<&str> = a.split('-').collect();
        assert_eq!(parts.len(), 5);
        assert_eq!(parts[2].len(), 4);
        assert!(parts[2].starts_with('7'), "version 位应为 7：{a}");
        assert!(
            parts[3].starts_with('8')
                || parts[3].starts_with('9')
                || parts[3].starts_with('a')
                || parts[3].starts_with('b')
        );
    }

    #[test]
    fn time_ordered_id_differs_by_seq_and_tag() {
        assert_ne!(
            time_ordered_id("transactions", 1, 1_700_000_000_000),
            time_ordered_id("transactions", 2, 1_700_000_000_000)
        );
        assert_ne!(
            time_ordered_id("accounts", 1, 1_700_000_000_000),
            time_ordered_id("transactions", 1, 1_700_000_000_000)
        );
    }

    #[test]
    fn time_ordered_id_encodes_timestamp_order() {
        let earlier = time_ordered_id("t", 1, 1_700_000_000_000);
        let later = time_ordered_id("t", 2, 1_700_000_100_000);
        assert!(earlier < later, "时间位在前，ID 应随时间单调");
    }
}
