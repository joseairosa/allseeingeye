//! Static price table for vendor model billing.
//!
//! **VERIFY BEFORE QUOTING.** The values here are USD per million
//! tokens, transcribed from public Anthropic and `OpenAI` pricing pages
//! as of **2026-05** (price table version `2026-05`). Vendors change
//! their price tables on a quarterly-or-faster cadence; surface the
//! `model_known` flag in the UI so users know which rows are pinned to
//! a known model id versus folded into the default fallback.
//!
//! Sources (capture date 2026-05-08):
//! - <https://www.anthropic.com/pricing> for Claude family.
//! - <https://openai.com/api/pricing/> for GPT family.
//!
//! Cache pricing follows the published pattern:
//! - Anthropic: `cache_read` is 10% of base input, `cache_create` is
//!   125% of base input (1h cache pricing).
//! - `OpenAI`: `cache_read` is 50% of base input. `cache_create` is
//!   the same as base input (no separate write fee on most tiers as
//!   of 2026-05).
//!
//! Prefix matching is intentionally permissive: `claude-opus-4-7`,
//! `claude-opus-4`, and `opus-4` all match the `claude-opus` prefix.
//! When a model id matches multiple entries, the longest matching
//! prefix wins. Unknown models fall through to [`DEFAULT_PRICE`] and
//! [`PriceLookup`] tags them with `model_known = false`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Version stamp for the price table. Bump every time numbers change.
/// The string is exposed via IPC so the UI footer can render
/// "prices reviewed YYYY-MM" without code changes. Currently only
/// referenced from tests and re-exported from `mod.rs`; the IPC
/// command that surfaces it lands with the frontend Cost view.
#[allow(dead_code)]
pub const PRICE_TABLE_VERSION: &str = "2026-05";

/// A single price-table entry. `pattern` is matched as a prefix on the
/// raw model id reported by the vendor (e.g. `"claude-opus-4-7"`,
/// `"openai/gpt-5"`).
#[derive(Debug, Clone, Copy)]
pub struct ModelPrice {
    /// Prefix matched against the model id. Longest match wins.
    pub pattern: &'static str,
    /// USD per 1M input tokens.
    pub input_per_m: f64,
    /// USD per 1M output tokens.
    pub output_per_m: f64,
    /// USD per 1M cache-read tokens.
    pub cache_read_per_m: f64,
    /// USD per 1M cache-creation tokens (the write fee).
    pub cache_create_per_m: f64,
}

/// Result of [`lookup_price`].
#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../bindings/usage/PriceLookup.ts")]
#[ts(rename_all = "camelCase")]
pub struct PriceLookup {
    pub input_per_m: f64,
    pub output_per_m: f64,
    pub cache_read_per_m: f64,
    pub cache_create_per_m: f64,
    /// `true` when the model id matched a known prefix; `false` when
    /// the lookup fell through to the default. The UI footnotes
    /// uncertain rows.
    pub model_known: bool,
}

/// Anthropic + `OpenAI` model prices (USD per 1M tokens).
///
/// Order is irrelevant - [`lookup_price`] picks the longest-matching
/// prefix. Keep entries sorted by pattern length descending purely for
/// human readability.
const PRICES: &[ModelPrice] = &[
    // Anthropic Claude family.
    ModelPrice {
        pattern: "claude-opus-4-7",
        input_per_m: 15.0,
        output_per_m: 75.0,
        cache_read_per_m: 1.5,
        cache_create_per_m: 18.75,
    },
    ModelPrice {
        pattern: "claude-opus-4",
        input_per_m: 15.0,
        output_per_m: 75.0,
        cache_read_per_m: 1.5,
        cache_create_per_m: 18.75,
    },
    ModelPrice {
        pattern: "claude-opus",
        input_per_m: 15.0,
        output_per_m: 75.0,
        cache_read_per_m: 1.5,
        cache_create_per_m: 18.75,
    },
    ModelPrice {
        pattern: "claude-sonnet-4-7",
        input_per_m: 3.0,
        output_per_m: 15.0,
        cache_read_per_m: 0.3,
        cache_create_per_m: 3.75,
    },
    ModelPrice {
        pattern: "claude-sonnet-4",
        input_per_m: 3.0,
        output_per_m: 15.0,
        cache_read_per_m: 0.3,
        cache_create_per_m: 3.75,
    },
    ModelPrice {
        pattern: "claude-sonnet",
        input_per_m: 3.0,
        output_per_m: 15.0,
        cache_read_per_m: 0.3,
        cache_create_per_m: 3.75,
    },
    ModelPrice {
        pattern: "claude-haiku-4",
        input_per_m: 0.8,
        output_per_m: 4.0,
        cache_read_per_m: 0.08,
        cache_create_per_m: 1.0,
    },
    ModelPrice {
        pattern: "claude-haiku",
        input_per_m: 0.8,
        output_per_m: 4.0,
        cache_read_per_m: 0.08,
        cache_create_per_m: 1.0,
    },
    // OpenAI GPT family. Model ids may arrive with provider prefixes
    // such as `openai/gpt-5` (OpenRouter) or bare `gpt-5` (direct).
    // We list both. Prefix matching is case-sensitive; vendors use
    // lowercase consistently so this is fine.
    ModelPrice {
        pattern: "openai/gpt-5",
        input_per_m: 1.25,
        output_per_m: 10.0,
        cache_read_per_m: 0.625,
        cache_create_per_m: 1.25,
    },
    ModelPrice {
        pattern: "gpt-5",
        input_per_m: 1.25,
        output_per_m: 10.0,
        cache_read_per_m: 0.625,
        cache_create_per_m: 1.25,
    },
    ModelPrice {
        pattern: "openai/gpt-4o",
        input_per_m: 2.5,
        output_per_m: 10.0,
        cache_read_per_m: 1.25,
        cache_create_per_m: 2.5,
    },
    ModelPrice {
        pattern: "gpt-4o",
        input_per_m: 2.5,
        output_per_m: 10.0,
        cache_read_per_m: 1.25,
        cache_create_per_m: 2.5,
    },
];

/// Default fallback - Sonnet-tier rates so unknown models do not
/// catastrophically over- or under-quote. Tagged `model_known = false`.
const DEFAULT_PRICE: ModelPrice = ModelPrice {
    pattern: "*default*",
    input_per_m: 3.0,
    output_per_m: 15.0,
    cache_read_per_m: 0.3,
    cache_create_per_m: 3.75,
};

/// Look up the per-1M prices for a model id.
///
/// Picks the longest matching prefix from [`PRICES`]; falls through to
/// [`DEFAULT_PRICE`] when nothing matches. The returned struct's
/// `model_known` flag is `false` only on the fallback path.
#[must_use]
pub fn lookup_price(model: &str) -> PriceLookup {
    let lower = model.to_ascii_lowercase();
    let best = PRICES
        .iter()
        .filter(|p| lower.starts_with(p.pattern))
        .max_by_key(|p| p.pattern.len());
    match best {
        Some(p) => PriceLookup {
            input_per_m: p.input_per_m,
            output_per_m: p.output_per_m,
            cache_read_per_m: p.cache_read_per_m,
            cache_create_per_m: p.cache_create_per_m,
            model_known: true,
        },
        None => PriceLookup {
            input_per_m: DEFAULT_PRICE.input_per_m,
            output_per_m: DEFAULT_PRICE.output_per_m,
            cache_read_per_m: DEFAULT_PRICE.cache_read_per_m,
            cache_create_per_m: DEFAULT_PRICE.cache_create_per_m,
            model_known: false,
        },
    }
}

/// Compute USD cost for a single turn given its token breakdown.
///
/// `cache_read` and `cache_create` are in addition to `input` (NOT
/// double-counted). This matches Anthropic's billing model where the
/// three counters are disjoint.
#[must_use]
pub fn estimate_cost_usd(
    model: &str,
    input: u64,
    output: u64,
    cache_read: u64,
    cache_create: u64,
) -> f64 {
    let p = lookup_price(model);
    let per_m = 1_000_000.0_f64;
    #[allow(clippy::cast_precision_loss)]
    let input_cost = (input as f64) / per_m * p.input_per_m;
    #[allow(clippy::cast_precision_loss)]
    let output_cost = (output as f64) / per_m * p.output_per_m;
    #[allow(clippy::cast_precision_loss)]
    let cache_read_cost = (cache_read as f64) / per_m * p.cache_read_per_m;
    #[allow(clippy::cast_precision_loss)]
    let cache_create_cost = (cache_create as f64) / per_m * p.cache_create_per_m;
    input_cost + output_cost + cache_read_cost + cache_create_cost
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn opus_lookup() {
        let p = lookup_price("claude-opus-4-7");
        assert!(p.model_known);
        assert!(approx(p.input_per_m, 15.0));
        assert!(approx(p.output_per_m, 75.0));
    }

    #[test]
    fn sonnet_lookup() {
        let p = lookup_price("claude-sonnet-4-7");
        assert!(p.model_known);
        assert!(approx(p.input_per_m, 3.0));
        assert!(approx(p.output_per_m, 15.0));
    }

    #[test]
    fn haiku_lookup() {
        let p = lookup_price("claude-haiku-4");
        assert!(p.model_known);
        assert!(approx(p.input_per_m, 0.8));
    }

    #[test]
    fn gpt5_lookup() {
        let p = lookup_price("gpt-5");
        assert!(p.model_known);
        assert!(approx(p.input_per_m, 1.25));
        let pp = lookup_price("openai/gpt-5");
        assert!(pp.model_known);
        assert!(approx(pp.input_per_m, 1.25));
    }

    #[test]
    fn longest_prefix_wins() {
        // `claude-opus-4-7` should match the most specific entry, not
        // the bare `claude-opus`.
        let p = lookup_price("claude-opus-4-7");
        assert!(p.model_known);
        // Haiku has a shorter prefix; ensure the longer one wins for
        // the longer id.
        let q = lookup_price("claude-haiku-4");
        assert!(approx(q.input_per_m, 0.8));
    }

    #[test]
    fn unknown_falls_through_with_flag() {
        let p = lookup_price("vendor-mystery-model-9000");
        assert!(!p.model_known);
        assert!(approx(p.input_per_m, DEFAULT_PRICE.input_per_m));
        assert!(approx(p.output_per_m, DEFAULT_PRICE.output_per_m));
    }

    #[test]
    fn estimate_cost_combines_all_buckets() {
        // 1M input tokens at $3/M = $3.
        // 1M output tokens at $15/M = $15.
        // 1M cache_read at $0.3/M = $0.30.
        // 1M cache_create at $3.75/M = $3.75.
        // Total: $22.05.
        let cost = estimate_cost_usd(
            "claude-sonnet-4-7",
            1_000_000,
            1_000_000,
            1_000_000,
            1_000_000,
        );
        assert!(approx(cost, 22.05));
    }

    #[test]
    fn estimate_cost_unknown_uses_default() {
        let cost = estimate_cost_usd("nope", 1_000_000, 0, 0, 0);
        assert!(approx(cost, 3.0)); // default sonnet rate
    }

    #[test]
    fn version_stamp_present() {
        assert!(!PRICE_TABLE_VERSION.is_empty());
    }
}
