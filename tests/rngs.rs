//! Integration Test Crate

use rand::{Rng, RngCore, SeedableRng, TryRngCore};
use rand_chacha::ChaCha20Rng;
use rand_jitter_kernel::RandJitterKernel;
use rand_xoshiro::{SplitMix64, Xoshiro256PlusPlus};

#[test]
fn test_rngs() {
    let mut rng = RandJitterKernel::new().unwrap().unwrap_err();
    let mut chacha_rng = ChaCha20Rng::from_rng(&mut rng);

    for _ in 0..1024 {
        let _ = chacha_rng.next_u64();
    }

    for _ in 0..32 {
        let _: [u8; 32] = rng.random();
    }

    let mut xoshiro_rng = Xoshiro256PlusPlus::from_rng(&mut rng);
    for _ in 0..1024 {
        let _ = xoshiro_rng.next_u64();
    }

    let mut splitmix_rng = SplitMix64::from_rng(&mut rng);
    for _ in 0..1024 {
        let _ = splitmix_rng.next_u64();
    }
}
