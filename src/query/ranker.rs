//! Groups scored entities by file and produces a ranked RelevantFile list.
//!
//! The ranker sits between relevance.rs (per-entity scores) and api.rs
//! (the public QueryEngine). It handles three tasks:
//!
//!   1. Group entities by file_path
//!   2. Compute per-file score = max entity score in that file
//!   3. Build a human-readable reason string
//!   4. Sort by score descending and return top-K

use crate::indexer::extractor::CodeEntity;
use super::mod_types::RelevantFile;
use std::collections::HashMap;

/// A scored entity ready for ranking.
#[derive(Debug, Clone)]
pub struct ScoredEntity {
    pub entity: CodeEntity,
    /// Combined relevance score for this entity (0.0 to 1.0)
    pub score: f64,
    /// Human-readable reason for this score
    pub reason: String,
}

/// Groups `scored_entities` by file path, computes per-file scores,
/// sorts descending, and returns the top `top_k` results.
///
/// Per-file score = max entity score in that file.
///
/// Why max instead of average?
/// If a file has one highly relevant function (0.9) and nine irrelevant
/// ones (0.0), the file IS relevant -the one function is what matters.
/// Averaging would bury it at 0.09. Max correctly surfaces it.
pub fn rank_files(scored_entities: Vec<ScoredEntity>, top_k: usize) -> Vec<RelevantFile> {
    if scored_entities.is_empty() || top_k == 0 {
        return Vec::new();
    }

    // Group entities by file path.
    let mut by_file: HashMap<String, Vec<ScoredEntity>> = HashMap::new();
    for se in scored_entities {
        by_file.entry(se.entity.file_path.clone()).or_default().push(se);
    }

    // For each file, score = max entity score; reason comes from the best entity.
    let mut files: Vec<RelevantFile> = by_file.into_iter().map(|(file_path, entities)| {
        let best = entities.iter()
            .max_by(|a, b| a.score.partial_cmp(&b.score).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap(); // safe: entities is non-empty

        RelevantFile {
            file_path,
            relevance_score: best.score,
            reason: best.reason.clone(),
            entities: entities.into_iter().map(|se| se.entity).collect(),
            stale_warning: None, // populated by QueryEngine after ranking
        }
    }).collect();

    // Sort descending. Use partial_cmp because f64 requires it; our scores
    // are always in [0.0, 1.0] so NaN cannot appear in practice.
    files.sort_by(|a, b| {
        b.relevance_score
            .partial_cmp(&a.relevance_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    files.truncate(top_k);
    files
}

/// Builds a human-readable reason string for a scored entity.
///
/// Picks the most informative reason in priority order:
/// name match > graph distance > recency > docstring > fallback.
pub fn build_reason(entity: &CodeEntity, query: &str, distance: Option<usize>, seconds_ago: Option<f64>) -> String {
    let query_lower = query.to_lowercase();

    // Name match is the strongest and most interpretable signal.
    let name_lower = entity.name.to_lowercase();
    let name_matches = name_lower.contains(&query_lower)
        || query_lower.split_whitespace().any(|t| t.len() >= 2 && name_lower.contains(t));

    if name_matches {
        return format!("name match: {}", entity.name);
    }

    // Also check file path -e.g. query "src" matching "src/auth.rs"
    let path_lower = entity.file_path.to_lowercase();
    if path_lower.contains(&query_lower)
        || query_lower.split_whitespace().any(|t| t.len() >= 2 && path_lower.contains(t))
    {
        return format!("file path match: {}", entity.file_path);
    }

    // Graph distance is the next clearest explanation.
    if let Some(d) = distance {
        if d == 1 {
            return "directly connected to query match (1 hop)".to_string();
        } else if d > 1 {
            return format!("connected via call graph ({d} hops)");
        }
    }

    // Recency gives useful context even without a name match.
    if let Some(secs) = seconds_ago {
        if secs < 60.0 {
            return "recently accessed (< 1 minute ago)".to_string();
        } else if secs < 3600.0 {
            return format!("recently accessed ({} minutes ago)", (secs / 60.0) as u64);
        } else {
            return format!("recently accessed ({} hours ago)", (secs / 3600.0) as u64);
        }
    }

    // Docstring match as fallback.
    if let Some(doc) = &entity.docstring {
        if doc.to_lowercase().contains(&query_lower) {
            return format!("docstring mentions \"{}\"", query);
        }
    }

    "related via code structure".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indexer::extractor::{EntityType, Language};

    fn make_entity(file: &str, name: &str) -> CodeEntity {
        CodeEntity {
            id: format!("{file}::{name}"),
            name: name.to_string(),
            entity_type: EntityType::Function,
            file_path: file.to_string(),
            line_start: 1,
            line_end: 10,
            language: Language::Rust,
            docstring: None,
            calls: vec![],
            called_by: vec![],
        }
    }

    fn make_scored(file: &str, name: &str, score: f64) -> ScoredEntity {
        ScoredEntity {
            entity: make_entity(file, name),
            score,
            reason: format!("name match: {name}"),
        }
    }

    // ── rank_files ────────────────────────────────────────────────────────────

    #[test]
    fn rank_files_empty_input() {
        assert!(rank_files(vec![], 10).is_empty());
    }

    #[test]
    fn rank_files_top_k_zero() {
        let se = make_scored("a.rs", "foo", 0.9);
        assert!(rank_files(vec![se], 0).is_empty());
    }

    #[test]
    fn rank_files_single_entity() {
        let se = make_scored("a.rs", "foo", 0.8);
        let results = rank_files(vec![se], 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file_path, "a.rs");
        assert!((results[0].relevance_score - 0.8).abs() < f64::EPSILON);
    }

    #[test]
    fn rank_files_two_entities_same_file() {
        let se1 = make_scored("a.rs", "foo", 0.8);
        let se2 = make_scored("a.rs", "bar", 0.3);
        let results = rank_files(vec![se1, se2], 10);
        // Two entities in same file → one RelevantFile
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file_path, "a.rs");
    }

    #[test]
    fn rank_files_two_entities_different_files() {
        let se1 = make_scored("a.rs", "foo", 0.8);
        let se2 = make_scored("b.rs", "bar", 0.3);
        let results = rank_files(vec![se1, se2], 10);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn rank_files_score_is_max_not_average() {
        // File has scores [0.9, 0.1] → file score must be 0.9
        let se1 = make_scored("a.rs", "high", 0.9);
        let se2 = make_scored("a.rs", "low",  0.1);
        let results = rank_files(vec![se1, se2], 10);
        assert_eq!(results.len(), 1);
        assert!((results[0].relevance_score - 0.9).abs() < f64::EPSILON);
    }

    #[test]
    fn rank_files_sorted_descending() {
        let se1 = make_scored("a.rs", "low",  0.2);
        let se2 = make_scored("b.rs", "high", 0.9);
        let se3 = make_scored("c.rs", "mid",  0.5);
        let results = rank_files(vec![se1, se2, se3], 10);
        assert_eq!(results.len(), 3);
        assert!(results[0].relevance_score >= results[1].relevance_score);
        assert!(results[1].relevance_score >= results[2].relevance_score);
        assert_eq!(results[0].file_path, "b.rs");
    }

    #[test]
    fn rank_files_top_k_truncates() {
        let se1 = make_scored("a.rs", "a", 0.9);
        let se2 = make_scored("b.rs", "b", 0.7);
        let se3 = make_scored("c.rs", "c", 0.5);
        let results = rank_files(vec![se1, se2, se3], 1);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file_path, "a.rs");
    }

    #[test]
    fn rank_files_top_k_larger_than_count() {
        let se1 = make_scored("a.rs", "a", 0.9);
        let se2 = make_scored("b.rs", "b", 0.5);
        let results = rank_files(vec![se1, se2], 100);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn rank_files_entities_list_contains_all_for_file() {
        let se1 = make_scored("a.rs", "foo", 0.8);
        let se2 = make_scored("a.rs", "bar", 0.3);
        let results = rank_files(vec![se1, se2], 10);
        assert_eq!(results[0].entities.len(), 2);
    }

    #[test]
    fn rank_files_reason_from_best_entity() {
        let mut se1 = make_scored("a.rs", "low",  0.2);
        se1.reason = "low-score reason".to_string();
        let mut se2 = make_scored("a.rs", "high", 0.9);
        se2.reason = "high-score reason".to_string();
        let results = rank_files(vec![se1, se2], 10);
        // Reason should come from the entity with score 0.9
        assert_eq!(results[0].reason, "high-score reason");
    }

    #[test]
    fn rank_files_equal_scores_both_present() {
        // Equal scores: both files must appear, order is implementation-defined
        let se1 = make_scored("a.rs", "a", 0.5);
        let se2 = make_scored("b.rs", "b", 0.5);
        let results = rank_files(vec![se1, se2], 10);
        assert_eq!(results.len(), 2);
        let paths: Vec<&str> = results.iter().map(|r| r.file_path.as_str()).collect();
        assert!(paths.contains(&"a.rs"));
        assert!(paths.contains(&"b.rs"));
    }

    // ── build_reason ──────────────────────────────────────────────────────────

    #[test]
    fn build_reason_name_match_contains_name() {
        let e = make_entity("src/auth.rs", "authenticate_user");
        let r = build_reason(&e, "authenticate", None, None);
        assert!(r.contains("authenticate_user"), "got: {r}");
    }

    #[test]
    fn build_reason_graph_distance_one_hop() {
        let e = make_entity("src/other.rs", "unrelated_fn");
        let r = build_reason(&e, "authenticate", Some(1), None);
        assert!(r.contains("1 hop"), "got: {r}");
    }

    #[test]
    fn build_reason_recency_under_60s() {
        let e = make_entity("src/other.rs", "unrelated_fn");
        let r = build_reason(&e, "authenticate", None, Some(30.0));
        assert!(r.contains("< 1 minute"), "got: {r}");
    }

    #[test]
    fn build_reason_recency_minutes() {
        let e = make_entity("src/other.rs", "unrelated_fn");
        // 30 minutes = 1800s -below the 3600s threshold, so "minutes ago" branch
        let r = build_reason(&e, "authenticate", None, Some(30.0 * 60.0));
        assert!(r.contains("minutes ago"), "got: {r}");
    }

    #[test]
    fn build_reason_docstring_match() {
        let mut e = make_entity("src/other.rs", "unrelated");
        // Docstring contains the query word exactly -not "authentication" (different word)
        e.docstring = Some("Handles authenticate calls".into());
        let r = build_reason(&e, "authenticate", None, None);
        assert!(r.contains("authenticate") || r.contains("docstring"), "got: {r}");
    }

    #[test]
    fn build_reason_fallback_non_empty() {
        let e = make_entity("src/other.rs", "completely_unrelated");
        let r = build_reason(&e, "authenticate", None, None);
        assert!(!r.is_empty());
    }
}
