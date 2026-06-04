//! Relevance scoring functions for the GraphSwarm query engine.
//!
//! Each function scores one signal and returns a value in [0.0, 1.0].
//! The QueryEngine combines them with fixed weights into a final score.
//!
//! Design principle: every function is PURE — no side effects, no I/O.
//! This makes them trivially testable and easy to reason about.
//!
//! The four signals:
//!   name_score      — text match between query tokens and entity names
//!   graph_score     — graph distance from query-matching entities
//!   recency_score   — how recently the agent accessed this file
//!   docstring_score — text match in docstrings

/// Scores how well an entity's name matches the query tokens.
///
/// Algorithm:
/// 1. Tokenize query by whitespace and common delimiters (_, -, ::, ., /)
/// 2. Tokenize entity name the same way
/// 3. Score = (matched tokens) / (total query tokens)
///
/// Why token matching instead of substring search?
/// "authenticate user" should match "authenticate_user" even though
/// the query has a space and the name has an underscore. Tokenizing
/// both sides normalizes this difference.
///
/// Score 1.0 = every query token found in entity name (exact match)
/// Score 0.5 = half the query tokens found
/// Score 0.0 = no query tokens found in entity name
///
/// Case-insensitive: "AUTH" matches "authenticate_user"
pub fn name_score(entity_name: &str, query: &str) -> f64 {
    let query_tokens = tokenize(query);

    if query_tokens.is_empty() {
        return 0.0;
    }

    let name_tokens = tokenize(entity_name);
    let name_lower = entity_name.to_lowercase();

    // Count how many query tokens appear anywhere in the entity name.
    // We check substring containment: "auth" matches "authenticate".
    let matched = query_tokens.iter().filter(|qt| {
        name_tokens.iter().any(|nt| nt.contains(qt.as_str()))
            || name_lower.contains(qt.as_str())
    }).count();

    matched as f64 / query_tokens.len() as f64
}

/// Scores a file based on its graph distance from query-matching entities.
///
/// `min_distance` is the fewest hops from this file to the nearest entity
/// that scored highly on name_score.
///
/// Distance 0 = the file directly contains a name-matching entity → 1.0
/// Distance 1 = the file calls or is called by a matching entity  → 0.7
/// Distance 2 = two hops away                                     → 0.4
/// Distance 3 = three hops away                                   → 0.2
/// Distance 4+ = too far to be meaningfully related               → 0.0
///
/// Why exponential decay?
/// Relevance drops sharply with graph distance. A direct caller is very
/// likely relevant. A caller of a caller is maybe relevant. Anything
/// further is usually coincidental.
pub fn graph_score(min_distance: usize) -> f64 {
    // Each step approximately halves the score.
    match min_distance {
        0 => 1.0,
        1 => 0.7,
        2 => 0.4,
        3 => 0.2,
        _ => 0.0,
    }
}

/// Scores a file based on how recently the agent accessed it.
///
/// Uses half-life decay: score = 0.5^(elapsed / half_life)
///
/// Half-life = 3600 seconds (1 hour):
///   0 seconds ago  → 1.0  (just now)
///   1 hour ago     → 0.5
///   2 hours ago    → 0.25
///   1 day ago      → ~0.003 (effectively 0)
///
/// Why half-life decay instead of a step function?
/// A step function creates sharp discontinuities. Smooth decay gives
/// more sensible rankings when files were accessed minutes apart.
///
/// If the file was never accessed: returns 0.0
pub fn recency_score(seconds_since_access: Option<f64>) -> f64 {
    match seconds_since_access {
        None => 0.0,
        Some(elapsed) => {
            // 3600s = 1 hour; score halves every hour.
            const HALF_LIFE: f64 = 3600.0;
            (0.5_f64).powf(elapsed / HALF_LIFE)
        }
    }
}

/// Scores how well query tokens match an entity's docstring.
///
/// Same algorithm as name_score but applied to the docstring text.
/// Returns 0.0 if the entity has no docstring.
///
/// Why lower weight (0.1) than name_score (0.4)?
/// Docstrings are often missing, outdated, or use different terminology.
/// Names are a much more reliable signal.
pub fn docstring_score(docstring: Option<&str>, query: &str) -> f64 {
    let doc = match docstring {
        None => return 0.0,
        Some("") => return 0.0,
        Some(d) => d,
    };

    let query_tokens = tokenize(query);
    if query_tokens.is_empty() {
        return 0.0;
    }

    let doc_lower = doc.to_lowercase();
    let matched = query_tokens.iter()
        .filter(|qt| doc_lower.contains(qt.as_str()))
        .count();

    (matched as f64 / query_tokens.len() as f64).min(1.0)
}

/// Tokenizes a string into lowercase words by splitting on:
/// whitespace, underscores, hyphens, colons, dots, slashes.
///
/// "authenticate_user" → ["authenticate", "user"]
/// "src/auth.rs"       → ["src", "auth", "rs"]
///
/// Filters out empty tokens and tokens shorter than 2 chars (noise).
pub(crate) fn tokenize(s: &str) -> Vec<String> {
    s.split(|c: char| c.is_whitespace() || matches!(c, '_' | '-' | ':' | '.' | '/'))
        .map(|t| t.to_lowercase())
        .filter(|t| t.len() >= 2)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── name_score ────────────────────────────────────────────────────────────

    #[test]
    fn name_score_exact_match() {
        // "authenticate user" tokenizes to ["authenticate","user"]
        // "authenticate_user" also tokenizes to those two — full match
        let s = name_score("authenticate_user", "authenticate user");
        assert!((s - 1.0).abs() < f64::EPSILON, "expected 1.0, got {s}");
    }

    #[test]
    fn name_score_partial_match() {
        // "authenticate" matches, "unknown" does not → 0.5
        let s = name_score("authenticate_user", "authenticate unknown");
        assert!((s - 0.5).abs() < f64::EPSILON, "expected 0.5, got {s}");
    }

    #[test]
    fn name_score_no_match() {
        let s = name_score("render_page", "authenticate user");
        assert!((s - 0.0).abs() < f64::EPSILON, "expected 0.0, got {s}");
    }

    #[test]
    fn name_score_case_insensitive() {
        // "AUTH" lowercased to "auth" is a substring of "authenticate_user"
        let s = name_score("authenticate_user", "AUTH");
        assert!(s > 0.0, "expected > 0.0, got {s}");
    }

    #[test]
    fn name_score_empty_query() {
        let s = name_score("authenticate_user", "");
        assert!((s - 0.0).abs() < f64::EPSILON, "empty query must give 0.0");
    }

    #[test]
    fn name_score_substring_match() {
        // "auth" is a substring of "authenticate"
        let s = name_score("authenticate_user", "auth");
        assert!(s > 0.0, "substring match must give > 0.0, got {s}");
    }

    #[test]
    fn name_score_multi_token_all_match() {
        let s = name_score("process_payment", "process payment");
        assert!((s - 1.0).abs() < f64::EPSILON, "expected 1.0, got {s}");
    }

    #[test]
    fn name_score_whitespace_only_query() {
        // Whitespace-only tokenizes to nothing → 0.0
        let s = name_score("authenticate_user", "   ");
        assert!((s - 0.0).abs() < f64::EPSILON);
    }

    // ── graph_score ───────────────────────────────────────────────────────────

    #[test]
    fn graph_score_distance_0() {
        assert!((graph_score(0) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn graph_score_distance_1() {
        assert!((graph_score(1) - 0.7).abs() < f64::EPSILON);
    }

    #[test]
    fn graph_score_distance_2() {
        assert!((graph_score(2) - 0.4).abs() < f64::EPSILON);
    }

    #[test]
    fn graph_score_distance_3() {
        assert!((graph_score(3) - 0.2).abs() < f64::EPSILON);
    }

    #[test]
    fn graph_score_distance_4() {
        assert!((graph_score(4) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn graph_score_large_distance() {
        assert!((graph_score(100) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn graph_score_strictly_decreasing() {
        assert!(graph_score(0) > graph_score(1));
        assert!(graph_score(1) > graph_score(2));
        assert!(graph_score(2) > graph_score(3));
        assert!(graph_score(3) > graph_score(4));
    }

    // ── recency_score ─────────────────────────────────────────────────────────

    #[test]
    fn recency_score_none_is_zero() {
        assert!((recency_score(None) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn recency_score_zero_seconds_is_one() {
        let s = recency_score(Some(0.0));
        assert!((s - 1.0).abs() < f64::EPSILON, "expected 1.0, got {s}");
    }

    #[test]
    fn recency_score_one_half_life() {
        // 3600 seconds = exactly 1 half-life → score = 0.5
        let s = recency_score(Some(3600.0));
        assert!((s - 0.5).abs() < 1e-10, "expected ~0.5, got {s}");
    }

    #[test]
    fn recency_score_two_half_lives() {
        // 7200 seconds = 2 half-lives → score = 0.25
        let s = recency_score(Some(7200.0));
        assert!((s - 0.25).abs() < 1e-10, "expected ~0.25, got {s}");
    }

    #[test]
    fn recency_score_strictly_decreasing() {
        let s0 = recency_score(Some(0.0));
        let s1 = recency_score(Some(1800.0));  // 30 min
        let s2 = recency_score(Some(3600.0));  // 1 hour
        let s3 = recency_score(Some(7200.0));  // 2 hours
        assert!(s0 > s1, "0s should beat 30min");
        assert!(s1 > s2, "30min should beat 1hr");
        assert!(s2 > s3, "1hr should beat 2hr");
    }

    #[test]
    fn recency_score_large_elapsed_near_zero() {
        // 30 days = 720 half-lives → 0.5^720 ≈ 2e-217, well above f64 zero
        let thirty_days = 30.0 * 24.0 * 3600.0;
        let s = recency_score(Some(thirty_days));
        assert!(s > 0.0, "must be strictly > 0 at 30 days");
        assert!(s < 1e-100, "must be negligibly small at 30 days, got {s}");
    }

    // ── docstring_score ───────────────────────────────────────────────────────

    #[test]
    fn docstring_score_none_is_zero() {
        assert!((docstring_score(None, "authenticate") - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn docstring_score_empty_string_is_zero() {
        assert!((docstring_score(Some(""), "authenticate") - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn docstring_score_match_returns_positive() {
        let s = docstring_score(Some("Authenticates a user by JWT token"), "authenticate");
        assert!(s > 0.0, "expected > 0.0, got {s}");
    }

    #[test]
    fn docstring_score_no_match() {
        let s = docstring_score(Some("Renders the page with CSS"), "authenticate");
        assert!((s - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn docstring_score_case_insensitive() {
        let s = docstring_score(Some("Authenticates a user"), "AUTH");
        assert!(s > 0.0, "case-insensitive match must give > 0.0");
    }

    #[test]
    fn docstring_score_capped_at_one() {
        // All query tokens match → score exactly 1.0, never above
        let s = docstring_score(Some("authenticate user session token"), "authenticate user");
        assert!(s <= 1.0, "score must not exceed 1.0, got {s}");
        assert!(s > 0.0);
    }

    // ── tokenize (via name_score) ─────────────────────────────────────────────

    #[test]
    fn tokenize_underscores_split() {
        // "process_payment" → ["process", "payment"] → both match "process payment"
        let s = name_score("process_payment", "process payment");
        assert!((s - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn tokenize_colons_split() {
        // Entity id "src::auth" should tokenize such that "auth" matches "auth"
        let s = name_score("src::auth", "auth");
        assert!(s > 0.0, "colon splitting must expose 'auth' token");
    }

    #[test]
    fn tokenize_short_tokens_filtered() {
        // Query token "a" is only 1 char → filtered → effectively empty query → 0.0
        let s = name_score("authenticate", "a");
        // "a" is filtered (len < 2) → query_tokens empty → 0.0
        assert!((s - 0.0).abs() < f64::EPSILON, "single-char token must be filtered");
    }
}
