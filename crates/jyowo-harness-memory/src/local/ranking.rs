//! Hybrid ranking for memory recall.
//!
//! Combines lexical score (FTS5), vector similarity, confidence, recency,
//! access history, source trust, and explicit selection boost.

use chrono::{DateTime, Utc};

/// Score breakdown produced during ranking.
#[derive(Debug, Clone, Default)]
pub struct RankScore {
    pub lexical_score: f32,
    pub vector_score: Option<f32>,
    pub confidence_score: f32,
    pub recency_score: f32,
    pub access_score: f32,
    pub source_trust_score: f32,
    pub explicit_selection_boost: f32,
    pub final_score: f32,
}

/// Normalize an FTS5 rank value to [0.0, 1.0].
///
/// SQLite FTS5 `rank` is a negative integer (closer to 0 = better match).
/// We convert it to a normalized score.
pub fn normalize_fts_rank(fts_rank: f64) -> f32 {
    // FTS5 rank is negative; larger (closer to 0) = better.
    // Normalize: score = 1.0 / (1.0 - fts_rank / scale)
    // This maps [-∞, 0] → [0.0, 1.0] with diminishing returns for poor matches.
    if fts_rank >= 0.0 {
        return 0.0;
    }
    let scale = 10.0;
    let normalized = 1.0 / (1.0 - fts_rank / scale);
    (normalized as f32).clamp(0.0, 1.0)
}

/// Compute recency score from `[0.0, 1.0]`.
///
/// Newer records score higher. Uses exponential decay with a 30-day half-life.
pub fn recency_score(updated_at: DateTime<Utc>, now: DateTime<Utc>) -> f32 {
    let age_hours = (now - updated_at).num_hours().max(0) as f64;
    let half_life_hours: f64 = 30.0 * 24.0; // 30 days
    let score = 0.5_f64.powf(age_hours / half_life_hours);
    score as f32
}

/// Compute access score from `[0.0, 1.0]`.
///
/// Frequently accessed records score slightly higher.
pub fn access_score(access_count: u32) -> f32 {
    if access_count == 0 {
        return 0.0;
    }
    // Logarithmic scaling: log2(access_count + 1) / log2(101)
    let score = f64::from(access_count + 1).log2() / 101.0_f64.log2();
    (score as f32).clamp(0.0, 1.0)
}

/// Compute the final hybrid ranking score using the formula from the plan:
///
/// ```text
/// final_score =
///   0.45 * lexical_score
///   + 0.30 * vector_score_or_0
///   + 0.10 * confidence_score
///   + 0.05 * recency_score
///   + 0.05 * source_trust_score
///   + 0.03 * explicit_selection_boost
///   + 0.02 * access_score
/// ```
///
/// If no vector score exists, weights are not renormalized.
pub fn compute_final_score(score: &RankScore) -> f32 {
    0.45 * score.lexical_score
        + 0.30 * score.vector_score.unwrap_or(0.0)
        + 0.10 * score.confidence_score
        + 0.05 * score.recency_score
        + 0.05 * score.source_trust_score
        + 0.03 * score.explicit_selection_boost
        + 0.02 * score.access_score
}
