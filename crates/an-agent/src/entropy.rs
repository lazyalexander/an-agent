use rand::rngs::{OsRng, StdRng};
use rand::{RngCore, SeedableRng};
use ulid::Ulid;

/// Two kinds of randomness. Os is production entropy; Seeded is ChaCha12 (rand StdRng).
pub enum Entropy {
    Os,
    Seeded(StdRng),
}

impl Entropy {
    pub fn os() -> Self {
        Self::Os
    }

    pub fn seeded(seed: [u8; 32]) -> Self {
        Self::Seeded(StdRng::from_seed(seed))
    }

    pub fn fill(&mut self, dest: &mut [u8]) {
        match self {
            Self::Os => OsRng.fill_bytes(dest),
            Self::Seeded(rng) => rng.fill_bytes(dest),
        }
    }

    /// 48-bit time from `now_ms` plus 80 bits from this source.
    pub fn ulid(&mut self, now_ms: u64) -> Ulid {
        let mut bytes = [0u8; 10];
        self.fill(&mut bytes);
        let mut rnd = 0u128;
        for b in bytes {
            rnd = (rnd << 8) | u128::from(b);
        }
        Ulid::from_parts(now_ms, rnd)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeded_is_reproducible() {
        let seed = [7u8; 32];
        let mut a = Entropy::seeded(seed);
        let mut b = Entropy::seeded(seed);
        assert_eq!(a.ulid(0).to_string(), b.ulid(0).to_string());
        let mut c = Entropy::seeded(seed);
        assert_ne!(c.ulid(0).to_string(), c.ulid(0).to_string());
    }

    #[test]
    fn os_and_seeded_are_distinct_kinds() {
        assert!(matches!(Entropy::os(), Entropy::Os));
        assert!(matches!(Entropy::seeded([1; 32]), Entropy::Seeded(_)));
    }
}
