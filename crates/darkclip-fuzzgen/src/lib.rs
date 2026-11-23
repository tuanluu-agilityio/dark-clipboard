//! Test-only support for the stable, deterministic fuzz harness (Task 406).
//!
//! This machine has no nightly toolchain, so libFuzzer (`cargo fuzz`, see `fuzz/`) cannot run
//! here. The fallback is a seeded generator plus structure-aware mutations, driven by
//! [`Fuzz::run`] from `#[ignore]` tests in each crate. It is a dev-dependency only: no product
//! crate depends on it in a normal build.
//!
//! What the runner checks for every input: the target does not panic, its live heap growth
//! during the call stays under a per-target budget (measured by [`TrackingAlloc`], which each
//! harness test binary installs as its global allocator), and the call finishes within a time
//! limit. A finding saves the input under `target/fuzz-findings/` and fails the test with the
//! target name, the seed and the iteration; the input itself is never printed (inputs are
//! synthetic, but the habit matches GC-06).
//!
//! Environment:
//! - `DARKCLIP_FUZZ_ITERS`: iterations per target (default 1,000,000).
//! - `DARKCLIP_FUZZ_SECS`: keep going until at least this many seconds have passed (default 0).
//! - `DARKCLIP_FUZZ_SEED`: base seed (default fixed); each target mixes in its own name.
//! - `DARKCLIP_FUZZ_SAVE_LAST=1`: write every input to `target/fuzz-findings/<name>.last`
//!   before the call, for a crash that aborts the process (for example an allocation
//!   failure). Slow; use it only to reproduce.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod alloc;
mod mutate;
pub mod png;

pub use alloc::{arm, disarm, TrackingAlloc};
pub use mutate::{crc32, fix_png_crcs, mutate};

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// SplitMix64: small, fast and fully deterministic for a given seed.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    pub fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    /// Uniform in `0..n`; 0 when `n` is 0.
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            return 0;
        }
        (self.next_u64() % n as u64) as usize
    }

    /// Uniform in `lo..=hi` (`lo` when `hi < lo`).
    pub fn range(&mut self, lo: usize, hi: usize) -> usize {
        lo + self.below(hi.saturating_sub(lo).saturating_add(1))
    }

    /// True with probability `1/n`.
    pub fn one_in(&mut self, n: usize) -> bool {
        self.below(n) == 0
    }

    pub fn bytes(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| self.next_u64() as u8).collect()
    }

    /// A random element of `items` (`None` when empty).
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        items.get(self.below(items.len()))
    }
}

/// One fuzz target for the stable harness.
pub struct Fuzz<'a> {
    /// Target name: also the `fuzz/corpus/<name>/` seed folder and the cargo-fuzz target.
    pub name: &'static str,
    /// Largest input handed to the target.
    pub max_len: usize,
    /// Tokens the mutator inserts or overwrites with (format keywords, magic numbers).
    pub dict: &'a [&'a [u8]],
    /// Most live heap growth one call may reach, in bytes.
    pub mem_budget: usize,
    /// Longest one call may take.
    pub time_limit: Duration,
    /// Applied after mutation to most inputs (for example: recompute PNG chunk CRCs so the
    /// mutations reach past the checksum).
    pub fixup: Option<fn(&mut Vec<u8>)>,
}

/// What a run did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub iterations: u64,
    pub seed: u64,
    pub elapsed: Duration,
    pub slowest_call: Duration,
    pub peak_heap: usize,
}

fn env_u64(key: &str) -> Option<u64> {
    let v = std::env::var(key).ok()?;
    let v = v.trim().replace('_', "");
    match v.strip_prefix("0x") {
        Some(hex) => u64::from_str_radix(hex, 16).ok(),
        None => v.parse().ok(),
    }
}

fn workspace_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

/// The committed seed corpus of `name` (`fuzz/corpus/<name>/`), empty when absent.
pub fn load_corpus(name: &str) -> Vec<Vec<u8>> {
    let dir = workspace_dir().join("fuzz").join("corpus").join(name);
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort();
    paths
        .into_iter()
        .filter_map(|p| std::fs::read(p).ok())
        .collect()
}

/// Writes `inputs` as the seed corpus of `name` (`fuzz/corpus/<name>/seed-NN`), replacing
/// the files of that name. Used by the `#[ignore]` corpus writers.
pub fn write_corpus(name: &str, inputs: &[Vec<u8>]) -> std::io::Result<()> {
    let dir = workspace_dir().join("fuzz").join("corpus").join(name);
    std::fs::create_dir_all(&dir)?;
    for (i, input) in inputs.iter().enumerate() {
        std::fs::write(dir.join(format!("seed-{i:02}")), input)?;
    }
    Ok(())
}

fn findings_dir() -> PathBuf {
    workspace_dir().join("target").join("fuzz-findings")
}

fn save_finding(name: &str, tag: &str, input: &[u8]) -> PathBuf {
    let dir = findings_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(format!("{name}-{tag}.bin"));
    let _ = std::fs::write(&path, input);
    path
}

fn name_hash(name: &str) -> u64 {
    // FNV-1a.
    name.bytes().fold(0xCBF2_9CE4_8422_2325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01B3)
    })
}

impl Fuzz<'_> {
    /// Runs the target over generated and mutated inputs. Each iteration starts from one of:
    /// a fresh input from `gen` (structure-aware), a committed corpus file, or an earlier
    /// mutated input (so mutations stack); then applies 0 to 8 mutations and the fixup.
    ///
    /// Fails (panics, as a test should) on the first finding, after saving the input.
    pub fn run(&self, mut gen: impl FnMut(&mut Rng) -> Vec<u8>, target: impl Fn(&[u8])) -> Summary {
        let iterations = env_u64("DARKCLIP_FUZZ_ITERS").unwrap_or(1_000_000);
        let min_time = Duration::from_secs(env_u64("DARKCLIP_FUZZ_SECS").unwrap_or(0));
        let seed = env_u64("DARKCLIP_FUZZ_SEED").unwrap_or(0x0406_DC1F) ^ name_hash(self.name);
        let save_last = std::env::var_os("DARKCLIP_FUZZ_SAVE_LAST").is_some();
        let corpus = load_corpus(self.name);
        let mut rng = Rng::new(seed);
        let mut pool: Vec<Vec<u8>> = Vec::new();
        const POOL: usize = 512;
        let start = Instant::now();
        let mut slowest = Duration::ZERO;
        let mut peak_heap = 0usize;
        let mut i: u64 = 0;
        while i < iterations || start.elapsed() < min_time {
            let mut input = match rng.below(8) {
                0 | 1 if !pool.is_empty() => pool[rng.below(pool.len())].clone(),
                2 if !corpus.is_empty() => corpus[rng.below(corpus.len())].clone(),
                _ => gen(&mut rng),
            };
            for _ in 0..rng.below(9) {
                mutate(&mut rng, &mut input, self.dict, self.max_len);
            }
            input.truncate(self.max_len);
            if let Some(fix) = self.fixup {
                if !rng.one_in(4) {
                    fix(&mut input);
                }
            }
            if rng.one_in(8) {
                if pool.len() < POOL {
                    pool.push(input.clone());
                } else {
                    let slot = rng.below(POOL);
                    pool[slot] = input.clone();
                }
            }
            if save_last {
                save_finding(self.name, "last", &input);
            }
            arm();
            let t0 = Instant::now();
            let outcome = catch_unwind(AssertUnwindSafe(|| target(&input)));
            let dt = t0.elapsed();
            let peak = disarm();
            if outcome.is_err() {
                let path = save_finding(self.name, &format!("panic-{seed:x}-{i}"), &input);
                panic_finding(self.name, "panicked", seed, i, &path);
            }
            if peak > self.mem_budget {
                let path = save_finding(self.name, &format!("heap-{seed:x}-{i}"), &input);
                panic_finding(self.name, "heap growth over budget", seed, i, &path);
            }
            if dt > self.time_limit {
                let path = save_finding(self.name, &format!("slow-{seed:x}-{i}"), &input);
                panic_finding(self.name, "call over time limit", seed, i, &path);
            }
            slowest = slowest.max(dt);
            peak_heap = peak_heap.max(peak);
            i += 1;
        }
        let summary = Summary {
            iterations: i,
            seed,
            elapsed: start.elapsed(),
            slowest_call: slowest,
            peak_heap,
        };
        println!(
            "fuzz {}: iterations={} seed={:#x} elapsed={:.1}s slowest_call={:.3}ms peak_heap={}B findings=0",
            self.name,
            summary.iterations,
            summary.seed,
            summary.elapsed.as_secs_f64(),
            summary.slowest_call.as_secs_f64() * 1000.0,
            summary.peak_heap
        );
        summary
    }
}

#[allow(clippy::panic)] // A finding must fail the calling test.
fn panic_finding(name: &str, what: &str, seed: u64, i: u64, path: &std::path::Path) -> ! {
    panic!(
        "fuzz {name}: {what} at iteration {i} (seed {seed:#x}); input saved to {}",
        path.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rng_is_deterministic() {
        let a: Vec<u64> = {
            let mut r = Rng::new(7);
            (0..4).map(|_| r.next_u64()).collect()
        };
        let mut r = Rng::new(7);
        let b: Vec<u64> = (0..4).map(|_| r.next_u64()).collect();
        assert_eq!(a, b);
        assert_ne!(a[0], a[1]);
        assert_eq!(Rng::new(1).below(0), 0);
        assert!((0..1000).all(|_| r.range(3, 5) >= 3 && r.range(3, 5) <= 5));
    }
}
