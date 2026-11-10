//! The price vector, and the decisions that follow from it.
//!
//! Most harness advice assumes input tokens are what you pay for. On the models this harness targets
//! that is false by one to two orders of magnitude: a cached input token on DeepSeek V4.1 Flash costs
//! 1/50 of a fresh one and 1/200 of an output token. So policy here is computed from prices instead of
//! being hard-coded: carrying context is nearly free, re-deliberating is not, and rewriting history is
//! only worth it when the arithmetic says so.

use crate::provider::Usage;
use crate::util::weekday;
use serde::{Deserialize, Serialize};

/// USD per million tokens.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Prices {
    /// Input served from the provider's prefix cache.
    pub hit: f64,
    /// Input that missed the cache.
    pub miss: f64,
    /// Output, including reasoning.
    pub out: f64,
}

/// List prices as published by the vendors (checked 2026-11-10). Gateways may differ; override in config.
pub fn prices_for(model: &str) -> Option<Prices> {
    let m = model.to_ascii_lowercase();
    let p = |hit, miss, out| Some(Prices { hit, miss, out });
    if m.contains("deepseek") {
        return if m.contains("pro") { p(0.022, 0.66, 1.98) } else { p(0.003, 0.15, 0.60) };
    }
    if m.contains("glm-5.3-flashx") {
        return p(0.075, 0.37, 1.25);
    }
    if m.contains("glm-5.3-flash") {
        return p(0.03, 0.15, 0.50);
    }
    if m.contains("glm-5.3") || m.contains("glm-5.2") || m.contains("glm-5.1") {
        return p(0.26, 1.40, 4.40);
    }
    if m.contains("glm-5") {
        return p(0.20, 1.00, 3.20);
    }
    if m.contains("glm-4") {
        return p(0.11, 0.60, 2.20);
    }
    None
}

/// DeepSeek bills double during 01:00–04:00 and 06:00–10:00 UTC on weekdays.
pub fn peak_multiplier(model: &str, unix_secs: u64) -> f64 {
    if !model.to_ascii_lowercase().contains("deepseek") || weekday(unix_secs) >= 5 {
        return 1.0;
    }
    let hour = (unix_secs % 86_400) / 3_600;
    if (1..4).contains(&hour) || (6..10).contains(&hour) { 2.0 } else { 1.0 }
}

impl Prices {
    pub fn cost(&self, u: &Usage) -> f64 {
        let fresh = u.prompt.saturating_sub(u.cached);
        (u.cached as f64 * self.hit + fresh as f64 * self.miss + u.output as f64 * self.out) / 1e6
    }

    /// How many fresh input tokens the harness may attach unasked to save one model turn.
    ///
    /// Attaching `n` tokens costs `n × miss`. If they turn out to be what the model would have asked
    /// for (probability `p_useful`) a turn of roughly `turn_output` output tokens is avoided.
    pub fn speculation_budget(&self, p_useful: f64, turn_output: f64) -> f64 {
        if self.miss <= 0.0 {
            return f64::INFINITY;
        }
        p_useful * turn_output * self.out / self.miss
    }

    /// Whether dropping `droppable` tokens of old observations pays for itself.
    ///
    /// Rewriting history invalidates the provider's cache from that point on, so the `suffix` tokens
    /// after the first rewritten message are re-bought once at the miss price. The saving is the
    /// dropped tokens no longer being re-read (at the hit price) on each remaining turn.
    pub fn decay_pays_off(&self, droppable: u64, suffix: u64, expected_turns: f64) -> bool {
        let saving = droppable as f64 * self.hit * expected_turns;
        let rebuy = suffix.saturating_sub(droppable) as f64 * (self.miss - self.hit);
        saving > rebuy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEEPSEEK: Prices = Prices { hit: 0.003, miss: 0.15, out: 0.60 };
    const GLM: Prices = Prices { hit: 0.26, miss: 1.40, out: 4.40 };

    #[test]
    fn known_models_resolve_and_unknown_do_not() {
        assert_eq!(prices_for("deepseek-v4.1-flash-1017"), Some(DEEPSEEK));
        assert_eq!(prices_for("economy/deepseek-v4.1-flash"), Some(DEEPSEEK));
        assert_eq!(prices_for("deepseek/deepseek-v4-pro").unwrap().out, 1.98);
        assert_eq!(prices_for("z-ai/glm-5.3"), Some(GLM));
        assert_eq!(prices_for("glm-5.3-flash").unwrap().out, 0.50);
        assert_eq!(prices_for("glm-5.3-flashx").unwrap().out, 1.25);
        assert_eq!(prices_for("kimi-k2.7-code"), None);
    }

    #[test]
    fn cost_splits_cached_and_fresh_input() {
        let u = Usage { prompt: 100_000, cached: 90_000, output: 2_000, reasoning: 1_500 };
        let c = DEEPSEEK.cost(&u);
        let expect = (90_000.0 * 0.003 + 10_000.0 * 0.15 + 2_000.0 * 0.60) / 1e6;
        assert!((c - expect).abs() < 1e-12);
        // Output dominates even though it is 2% of the tokens.
        assert!(2_000.0 * 0.60 > 90_000.0 * 0.003);
    }

    #[test]
    fn deepseek_peak_hours_are_weekday_utc_windows() {
        let tue_0230 = 1_794_339_533 - (1_794_339_533 % 86_400) + 2 * 3600 + 1800;
        assert_eq!(weekday(tue_0230), 1);
        assert_eq!(peak_multiplier("deepseek-flash", tue_0230), 2.0);
        assert_eq!(peak_multiplier("deepseek-flash", tue_0230 + 3 * 3600), 1.0); // 05:30
        assert_eq!(peak_multiplier("deepseek-flash", tue_0230 + 5 * 3600), 2.0); // 07:30
        assert_eq!(peak_multiplier("deepseek-flash", tue_0230 + 4 * 86_400), 1.0); // Saturday
        assert_eq!(peak_multiplier("glm-5.3", tue_0230), 1.0);
    }

    #[test]
    fn speculation_is_generous_when_output_is_dear() {
        // A 50% chance of saving a 1500-token turn justifies ~3000 unasked input tokens on both models.
        assert!((DEEPSEEK.speculation_budget(0.5, 1500.0) - 3000.0).abs() < 1e-6);
        assert!(GLM.speculation_budget(0.5, 1500.0) > 2300.0);
    }

    #[test]
    fn decay_never_pays_on_deepseek_but_does_on_glm() {
        // Drop 30k tokens of stale observations out of a 60k suffix.
        assert!(!DEEPSEEK.decay_pays_off(30_000, 60_000, 40.0), "cached input is too cheap to bother");
        assert!(GLM.decay_pays_off(30_000, 60_000, 12.0));
        assert!(!GLM.decay_pays_off(30_000, 60_000, 3.0), "too few turns left to recoup the cache bust");
    }
}
