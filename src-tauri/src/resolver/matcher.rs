use strsim::jaro_winkler;

/// Cleans track titles by stripping common streaming / video suffixes and noise.
pub fn clean_title(raw: &str) -> String {
    let lower = raw.to_lowercase();

    // Remove text inside parentheses/brackets if it contains video/audio/remaster junk
    let noise_patterns = [
        "(official video)",
        "[official video]",
        "(official audio)",
        "[official audio]",
        "(official music video)",
        "[official music video]",
        "(music video)",
        "[music video]",
        "(lyric video)",
        "[lyric video]",
        "(lyrics)",
        "[lyrics]",
        "(visualizer)",
        "[visualizer]",
        "(audio)",
        "[audio]",
        "(hd)",
        "[hd]",
        "(4k)",
        "[4k]",
        "(explicit)",
        "[explicit]",
        "(clean)",
        "[clean]",
        "(hq)",
        "[hq]",
    ];

    let mut cleaned = lower;
    for pattern in &noise_patterns {
        cleaned = cleaned.replace(pattern, " ");
    }

    // Strip out generic "(remastered 20xx)" or "(20xx remaster)"
    let re_remaster = regex::Regex::new(r"[\(\[](?:(?:\d{4}\s+)?remaster(?:ed)?(?:\s+\d{4})?)[\)\]]").unwrap();
    cleaned = re_remaster.replace_all(&cleaned, " ").to_string();

    // Strip featured artist suffixes like "(feat. ...)" or "[feat ...]"
    let re_feat = regex::Regex::new(r"[\(\[](?:feat|ft)\.?\s+[^\)\]]+[\)\]]").unwrap();
    cleaned = re_feat.replace_all(&cleaned, " ").to_string();

    // Remove extra punctuation and collapse whitespace
    let re_punct = regex::Regex::new(r"[^\w\s]").unwrap();
    let no_punct = re_punct.replace_all(&cleaned, " ");

    let re_ws = regex::Regex::new(r"\s+").unwrap();
    re_ws.replace_all(&no_punct, " ").trim().to_string()
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MatchEvaluation {
    pub confidence: f32,
    pub is_strict_match: bool,
    pub duration_delta_secs: f64,
    pub title_similarity: f32,
}

/// Evaluates how well a discovered candidate audio matches the target canonical track.
pub fn evaluate_track_match(
    target_title: &str,
    target_duration_ms: u64,
    cand_title: &str,
    cand_duration_ms: u64,
) -> MatchEvaluation {
    let clean_target = clean_title(target_title);
    let clean_cand = clean_title(cand_title);

    let title_similarity = if clean_target.is_empty() || clean_cand.is_empty() {
        0.0
    } else if clean_target == clean_cand {
        1.0
    } else {
        let jw = jaro_winkler(&clean_target, &clean_cand) as f32;
        // Substring / inclusion boost
        if clean_cand.contains(&clean_target) || clean_target.contains(&clean_cand) {
            jw.max(0.92)
        } else {
            jw
        }
    };

    let (duration_delta_secs, duration_score, duration_strict) = if target_duration_ms == 0 {
        // Target duration unknown; rely solely on title
        (0.0, 0.8, true)
    } else if cand_duration_ms == 0 {
        // Candidate duration unknown
        (0.0, 0.5, false)
    } else {
        let delta_ms = (cand_duration_ms as i64 - target_duration_ms as i64).abs();
        let delta_secs = (delta_ms as f64) / 1000.0;
        let score = if delta_secs <= 2.0 {
            1.0
        } else if delta_secs <= 3.5 {
            0.95
        } else if delta_secs <= 6.0 {
            0.75
        } else if delta_secs <= 12.0 {
            0.40
        } else {
            0.05
        };
        let is_strict = delta_secs <= 3.5;
        (delta_secs, score, is_strict)
    };

    let confidence = (title_similarity * 0.65 + duration_score * 0.35).clamp(0.0, 1.0);
    let is_strict_match = duration_strict && (title_similarity >= 0.82);

    MatchEvaluation {
        confidence,
        is_strict_match,
        duration_delta_secs,
        title_similarity,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_title_noise() {
        assert_eq!(
            clean_title("Acrid Avid Jam Shred (Official Audio)"),
            "acrid avid jam shred"
        );
        assert_eq!(
            clean_title("Ventolin (Video Edit) [Official Music Video]"),
            "ventolin video edit"
        );
        assert_eq!(
            clean_title("Alberto Balsalm (Remastered 2024)"),
            "alberto balsalm"
        );
        assert_eq!(
            clean_title("Song Name (feat. Kendrick Lamar) [HD]"),
            "song name"
        );
    }

    #[test]
    fn exact_match_has_high_confidence_and_strict_flag() {
        let eval = evaluate_track_match(
            "Acrid Avid Jam Shred",
            339000,
            "Acrid Avid Jam Shred",
            340000, // 1s difference
        );
        assert!(eval.is_strict_match);
        assert!(eval.confidence > 0.95);
        assert!(eval.duration_delta_secs <= 1.5);
    }

    #[test]
    fn title_with_youtube_junk_still_strict_matches() {
        let eval = evaluate_track_match(
            "Acrid Avid Jam Shred",
            339000,
            "Aphex Twin - Acrid Avid Jam Shred (Official Audio)",
            341000, // 2s difference
        );
        assert!(eval.is_strict_match);
        assert!(eval.confidence > 0.85);
    }

    #[test]
    fn duration_mismatch_fails_strict_match() {
        // e.g. extended 10-minute mix of a 4-minute track
        let eval = evaluate_track_match(
            "Alberto Balsalm",
            310000,
            "Alberto Balsalm (Extended Club Mix)",
            600000, // 5 minutes longer
        );
        assert!(!eval.is_strict_match);
        assert!(eval.confidence < 0.70);
    }
}
