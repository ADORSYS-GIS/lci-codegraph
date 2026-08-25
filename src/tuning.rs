//! Operator-tunable indexer knobs (ADR-0010 / epic #5), absorbed verbatim from
//! `agent-runner/src/indexer/mod.rs` so the embedding-path behaviour is byte-for-byte unchanged
//! (ADR-0086: "batching/round-trip tuning carries over verbatim"). Read from the environment once
//! per run and clamped to ≥1 so a misconfiguration can't wedge the pipeline.

/// Operator-tunable indexer knobs. These were hardcoded, which made a downstream limit (e.g. an
/// AI-gateway capping the batched-embedding *response* size) impossible to work around without a
/// rebuild — the reason this struct exists.
#[derive(Clone, Copy, Debug)]
pub struct IndexTuning {
    /// Chunks embedded + submitted per round-trip. Larger = fewer requests (kinder to per-minute
    /// rate limits) but a bigger embeddings response body, which some gateways cap.
    /// `LCI_CODEGRAPH_EMBED_BATCH_SIZE` (default 32).
    pub embed_batch_size: usize,
    /// Max lines a structured (tree-sitter) chunk may span before it is split into windows.
    /// `LCI_CODEGRAPH_MAX_CHUNK_LINES` (default 150).
    pub max_chunk_lines: usize,
    /// Windowed-fallback window size, in lines. `LCI_CODEGRAPH_WINDOW_SIZE` (default 100).
    pub window_size: usize,
    /// Windowed-fallback step, in lines (overlap = `window_size - window_step`).
    /// `LCI_CODEGRAPH_WINDOW_STEP` (default 50).
    pub window_step: usize,
}

impl Default for IndexTuning {
    fn default() -> Self {
        Self {
            embed_batch_size: 32,
            max_chunk_lines: 150,
            window_size: 100,
            window_step: 50,
        }
    }
}

impl IndexTuning {
    /// Read the knobs from the environment, falling back to [`Default`] and clamping each to ≥1.
    #[must_use]
    pub fn from_env() -> Self {
        let defaults = Self::default();
        Self {
            embed_batch_size: env_usize(
                "LCI_CODEGRAPH_EMBED_BATCH_SIZE",
                defaults.embed_batch_size,
            ),
            max_chunk_lines: env_usize("LCI_CODEGRAPH_MAX_CHUNK_LINES", defaults.max_chunk_lines),
            window_size: env_usize("LCI_CODEGRAPH_WINDOW_SIZE", defaults.window_size),
            window_step: env_usize("LCI_CODEGRAPH_WINDOW_STEP", defaults.window_step),
        }
    }
}

/// Parse a `usize` env var, clamping to ≥1; falls back to `default` when unset. A value that IS set
/// but fails to parse is surfaced via `tracing::warn!` rather than silently discarded — a typo'd or
/// out-of-range value should be diagnosable, not a silent no-op.
fn env_usize(key: &str, default: usize) -> usize {
    match std::env::var(key) {
        Ok(value) => match value.trim().parse::<usize>() {
            Ok(n) => n.max(1),
            Err(error) => {
                tracing::warn!(
                    key,
                    value,
                    %error,
                    "codegraph: unparseable tuning env var, falling back to default"
                );
                default
            }
        },
        Err(_) => default,
    }
}

#[cfg(test)]
mod tests {
    use super::{IndexTuning, env_usize};

    // `cargo test` runs this module's tests as threads within ONE process, and `std::env` is
    // process-global — so any two tests that set/read the same `INDEX_*` (or shared
    // `LCI_CODEGRAPH_TEST_ENV_USIZE*`) var concurrently can interleave and observe each other's
    // values. Every test below that touches process env acquires this for its whole body to
    // force those tests to run one at a time.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Restores a set of env vars to their original (possibly absent) values on drop, so a
    /// panicking assertion mid-test still cleans up rather than leaking mutated env into whatever
    /// test happens to run next in this process. Mirrors `tests/env_config.rs`'s `EnvGuard`.
    struct EnvGuard {
        saved: Vec<(&'static str, Option<String>)>,
    }

    impl EnvGuard {
        /// Captures the current value of each key (for restoration) without setting anything —
        /// for tests that only need cleanup, not an initial value.
        fn capture(keys: &[&'static str]) -> Self {
            let saved = keys.iter().map(|k| (*k, std::env::var(k).ok())).collect();
            Self { saved }
        }

        fn set(vars: &[(&'static str, &str)]) -> Self {
            let guard = Self::capture(&vars.iter().map(|(k, _)| *k).collect::<Vec<_>>());
            for (k, v) in vars {
                unsafe { std::env::set_var(k, v) };
            }
            guard
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (k, v) in &self.saved {
                match v {
                    Some(v) => unsafe { std::env::set_var(k, v) },
                    None => unsafe { std::env::remove_var(k) },
                }
            }
        }
    }

    #[test]
    fn defaults_match_the_historical_hardcoded_values() {
        let tuning = IndexTuning::default();
        assert_eq!(tuning.embed_batch_size, 32);
        assert_eq!(tuning.max_chunk_lines, 150);
        assert_eq!(tuning.window_size, 100);
        assert_eq!(tuning.window_step, 50);
    }

    #[test]
    fn env_usize_parses_clamps_to_one_and_falls_back() {
        // Poisoning here means some other test in this module already panicked and that failure
        // is already reported elsewhere; propagating the poison would just turn one real failure
        // into a cascade of spurious ones in every other locked test.
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // A test-unique key so this never races another test's environment.
        let key = "LCI_CODEGRAPH_TEST_ENV_USIZE";
        let _env_guard = EnvGuard::capture(&[key]);
        unsafe { std::env::remove_var(key) };
        assert_eq!(env_usize(key, 7), 7, "unset → default");
        unsafe { std::env::set_var(key, "12") };
        assert_eq!(env_usize(key, 7), 12, "parses a value");
        unsafe { std::env::set_var(key, "0") };
        assert_eq!(env_usize(key, 7), 1, "zero clamps to 1");
        unsafe { std::env::set_var(key, "not-a-number") };
        assert_eq!(env_usize(key, 7), 7, "unparseable → default");
    }

    #[test]
    fn env_usize_overflowing_value_falls_back_to_default() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // A numeral too large for `usize` fails to parse the same way non-numeric text does.
        let key = "LCI_CODEGRAPH_TEST_ENV_USIZE_OVERFLOW";
        let _env_guard = EnvGuard::capture(&[key]);
        unsafe { std::env::set_var(key, "999999999999999999999999999999999999") };
        assert_eq!(env_usize(key, 9), 9, "overflow parse failure → default");
    }

    #[test]
    fn from_env_reads_all_four_knobs() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let vars = [
            ("LCI_CODEGRAPH_EMBED_BATCH_SIZE", "16"),
            ("LCI_CODEGRAPH_MAX_CHUNK_LINES", "75"),
            ("LCI_CODEGRAPH_WINDOW_SIZE", "40"),
            ("LCI_CODEGRAPH_WINDOW_STEP", "20"),
        ];
        let _env_guard = EnvGuard::set(&vars);
        let tuning = IndexTuning::from_env();
        assert_eq!(tuning.embed_batch_size, 16);
        assert_eq!(tuning.max_chunk_lines, 75);
        assert_eq!(tuning.window_size, 40);
        assert_eq!(tuning.window_step, 20);
    }

    #[test]
    fn from_env_falls_back_to_defaults_when_unset() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let keys = [
            "LCI_CODEGRAPH_EMBED_BATCH_SIZE",
            "LCI_CODEGRAPH_MAX_CHUNK_LINES",
            "LCI_CODEGRAPH_WINDOW_SIZE",
            "LCI_CODEGRAPH_WINDOW_STEP",
        ];
        let _env_guard = EnvGuard::capture(&keys);
        for k in keys {
            unsafe { std::env::remove_var(k) };
        }
        let from_env = IndexTuning::from_env();
        let default = IndexTuning::default();
        // `IndexTuning` derives no `PartialEq` (kept as-is; adding one is out of scope for a test-only
        // change), so compare field-by-field.
        assert_eq!(from_env.embed_batch_size, default.embed_batch_size);
        assert_eq!(from_env.max_chunk_lines, default.max_chunk_lines);
        assert_eq!(from_env.window_size, default.window_size);
        assert_eq!(from_env.window_step, default.window_step);
    }
}
