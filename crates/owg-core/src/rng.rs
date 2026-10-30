use sha2::{Digest, Sha256};

/// Order-independent deterministic random source.
///
/// Every draw is a pure function of root seed, namespace, actor, time, and
/// draw index. Parallel scheduling therefore cannot silently change behavior.
#[derive(Clone, Copy, Debug)]
pub struct SeedBook {
    root_seed: u64,
}

impl SeedBook {
    #[must_use]
    pub const fn new(root_seed: u64) -> Self {
        Self { root_seed }
    }

    #[must_use]
    pub fn u64(self, namespace: &str, actor_id: u64, time_index: u32, draw_index: u32) -> u64 {
        let mut hasher = Sha256::new();
        hasher.update(b"openworldgen.seed-stream.v1");
        hasher.update(self.root_seed.to_le_bytes());
        hasher.update((namespace.len() as u64).to_le_bytes());
        hasher.update(namespace.as_bytes());
        hasher.update(actor_id.to_le_bytes());
        hasher.update(time_index.to_le_bytes());
        hasher.update(draw_index.to_le_bytes());
        let digest = hasher.finalize();
        u64::from_le_bytes(
            digest[..8]
                .try_into()
                .expect("SHA-256 digest always has at least eight bytes"),
        )
    }

    #[must_use]
    pub fn unit(self, namespace: &str, actor_id: u64, time_index: u32, draw_index: u32) -> f64 {
        let value = self.u64(namespace, actor_id, time_index, draw_index) >> 11;
        value as f64 * (1.0 / ((1_u64 << 53) as f64))
    }

    #[must_use]
    pub fn range(
        self,
        namespace: &str,
        actor_id: u64,
        time_index: u32,
        draw_index: u32,
        low: f64,
        high: f64,
    ) -> f64 {
        low + (high - low) * self.unit(namespace, actor_id, time_index, draw_index)
    }

    #[must_use]
    pub fn normal(self, namespace: &str, actor_id: u64, time_index: u32, draw_index: u32) -> f64 {
        let u1 = self
            .unit(namespace, actor_id, time_index, draw_index)
            .max(f64::MIN_POSITIVE);
        let u2 = self.unit(namespace, actor_id, time_index, draw_index + 1);
        (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
    }

    #[must_use]
    pub fn index(
        self,
        namespace: &str,
        actor_id: u64,
        time_index: u32,
        draw_index: u32,
        length: usize,
    ) -> usize {
        assert!(length > 0, "cannot sample from an empty collection");
        (self.u64(namespace, actor_id, time_index, draw_index) % length as u64) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::SeedBook;

    #[test]
    fn named_draws_are_replayable_and_isolated() {
        let seeds = SeedBook::new(42);
        let first = seeds.unit("household", 7, 12, 1);
        assert_eq!(first.to_bits(), seeds.unit("household", 7, 12, 1).to_bits());
        assert_ne!(first.to_bits(), seeds.unit("firm", 7, 12, 1).to_bits());
        assert_ne!(first.to_bits(), seeds.unit("household", 8, 12, 1).to_bits());
    }
}
