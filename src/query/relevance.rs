/// Relevance scoring components.
/// TODO: implement full algorithm in Part 4.
pub struct RelevanceScorer;

impl RelevanceScorer {
    /// Keyword overlap between file path/entities and task description.
    pub fn semantic_similarity(file: &str, task: &str) -> f32 {
        let keywords: Vec<&str> = task.split_whitespace().collect();
        if keywords.is_empty() { return 0.0; }
        let lower_file = file.to_lowercase();
        let matches = keywords.iter()
            .filter(|kw| lower_file.contains(&kw.to_lowercase()))
            .count();
        matches as f32 / keywords.len() as f32
    }

    /// Exponential decay: more recent = higher score.
    pub fn recency_score(seconds_ago: f64) -> f32 {
        let half_life_days = 7.0;
        let days = seconds_ago / 86_400.0;
        (-(days / half_life_days)).exp() as f32
    }

    /// Normalised error count.
    pub fn error_correlation(error_count: usize) -> f32 {
        (error_count as f32 / 10.0).min(1.0)
    }

    /// Normalised dependent count.
    pub fn dependency_importance(dependent_count: usize) -> f32 {
        (dependent_count as f32 / 20.0).min(1.0)
    }

    /// Weighted combination.
    pub fn combined(semantic: f32, recency: f32, errors: f32, deps: f32) -> f32 {
        semantic * 0.4 + recency * 0.3 + errors * 0.2 + deps * 0.1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_match() {
        let s = RelevanceScorer::semantic_similarity("payment.py", "Fix payment timeout");
        assert!(s > 0.0);
        let s2 = RelevanceScorer::semantic_similarity("auth.py", "Fix payment timeout");
        assert!(s > s2);
    }

    #[test]
    fn recency_decays() {
        let fresh = RelevanceScorer::recency_score(60.0);
        let old = RelevanceScorer::recency_score(86_400.0 * 30.0);
        assert!(fresh > old);
    }

    #[test]
    fn combined_range() {
        let c = RelevanceScorer::combined(1.0, 1.0, 1.0, 1.0);
        assert!((c - 1.0).abs() < f32::EPSILON);
    }
}
