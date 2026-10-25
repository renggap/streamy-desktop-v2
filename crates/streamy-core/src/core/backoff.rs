//! Backoff ladder and reconnect policy.
//!
//! Backoff ladder: 1s, 2s, 5s, 10s, then 20s cap — with +/-20% uniform random jitter,
//! clamped to [500, 24_000] ms. Randomized so concurrent clients don't reconnect in lockstep.

use rand::Rng;

pub const MAX_ATTEMPTS: u32 = 10;
pub const CAPACITY: usize = 64;

/// Compute next backoff sleep duration in milliseconds with uniform +/-20% jitter clamped to [500, 24000] ms.
pub fn next_backoff_ms<R: Rng + ?Sized>(attempt: u32, rng: &mut R) -> u64 {
    let raw = match attempt {
        0 | 1 => 1_000i64,
        2 => 2_000i64,
        3 => 5_000i64,
        4 => 10_000i64,
        _ => 20_000i64,
    };
    let jitter_pct: i64 = rng.random_range(-20..=20);
    let jitter = (raw * jitter_pct) / 100;
    (raw + jitter).clamp(500, 24_000) as u64
}

/// Whether the transport should give up reconnecting after `attempt` failures.
pub fn should_give_up(attempt: u32) -> bool {
    attempt >= MAX_ATTEMPTS
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    #[test]
    fn backoff_grows_then_caps_with_bounded_random_jitter() {
        let mut rng = StdRng::seed_from_u64(42);
        let delays: Vec<u64> = (1..=12).map(|att| next_backoff_ms(att, &mut rng)).collect();

        assert!(*delays.iter().max().unwrap() <= 24_000);
        assert!(*delays.iter().min().unwrap() >= 500);

        // Rung 1: 1s +/- 20% = [800, 1200]
        assert!(delays[0] >= 800 && delays[0] <= 1_200);

        // Rung 4: 10s +/- 20% = [8000, 12000] (at least >= 4000 and <= 12000)
        assert!(delays[3] >= 4_000 && delays[3] <= 12_000);

        // Rung 5+: 20s +/- 20% = [16000, 24000]
        assert!(delays[4] >= 16_000 && delays[4] <= 24_000);
        assert!(delays[11] <= 24_000);
    }

    #[test]
    fn jitter_is_random_across_runs_not_deterministic_by_attempt() {
        let mut rng1 = StdRng::seed_from_u64(1);
        let mut rng2 = StdRng::seed_from_u64(2);
        let a: Vec<u64> = (1..=12).map(|att| next_backoff_ms(att, &mut rng1)).collect();
        let b: Vec<u64> = (1..=12).map(|att| next_backoff_ms(att, &mut rng2)).collect();
        assert_ne!(a, b);
    }

    #[test]
    fn gives_up_after_max_attempts() {
        assert!(should_give_up(10));
        assert!(should_give_up(11));
        assert!(!should_give_up(9));
        assert!(!should_give_up(1));
    }
}
