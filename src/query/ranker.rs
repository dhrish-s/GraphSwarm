use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankedResult {
    pub file: String,
    pub score: f32,
    pub rank: usize,
}

pub struct ResultRanker;

impl ResultRanker {
    pub fn rank(mut items: Vec<(String, f32)>) -> Vec<RankedResult> {
        items.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        items.into_iter().enumerate().map(|(i, (file, score))| {
            RankedResult { file, score, rank: i + 1 }
        }).collect()
    }

    pub fn top_k(ranked: Vec<RankedResult>, k: usize) -> Vec<RankedResult> {
        ranked.into_iter().take(k).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rank_order() {
        let r = ResultRanker::rank(vec![
            ("a.py".into(), 0.3),
            ("b.py".into(), 0.9),
            ("c.py".into(), 0.6),
        ]);
        assert_eq!(r[0].file, "b.py");
        assert_eq!(r[0].rank, 1);
        assert_eq!(r[2].file, "a.py");
    }

    #[test]
    fn top_k_truncates() {
        let r = ResultRanker::rank(vec![
            ("a.py".into(), 0.1),
            ("b.py".into(), 0.2),
            ("c.py".into(), 0.3),
        ]);
        assert_eq!(ResultRanker::top_k(r, 2).len(), 2);
    }
}
