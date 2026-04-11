use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const RUBRIC_PASS_THRESHOLD: f64 = 0.75;
pub const LOW_CONFIDENCE_THRESHOLD: f64 = 0.70;
pub const NEAR_THRESHOLD_DELTA: f64 = 0.05;
pub const CALIBRATION_SCORE_TOLERANCE: f64 = 0.15;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvalRubricFamily {
    ResponseQuality,
}

impl EvalRubricFamily {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::ResponseQuality => "response_quality",
        }
    }

    pub const fn pass_threshold(&self) -> f64 {
        RUBRIC_PASS_THRESHOLD
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum EvalRubricDimension {
    Concision,
    Clarity,
    Groundedness,
    Completeness,
}

impl EvalRubricDimension {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Concision => "concision",
            Self::Clarity => "clarity",
            Self::Groundedness => "groundedness",
            Self::Completeness => "completeness",
        }
    }

    pub const fn all_for_family(family: EvalRubricFamily) -> &'static [Self] {
        match family {
            EvalRubricFamily::ResponseQuality => &[
                Self::Concision,
                Self::Clarity,
                Self::Groundedness,
                Self::Completeness,
            ],
        }
    }

    pub const fn minimum_score(&self) -> f64 {
        match self {
            Self::Concision => 0.60,
            Self::Clarity => 0.65,
            Self::Groundedness => 0.75,
            Self::Completeness => 0.65,
        }
    }

    pub const fn weight(&self, family: EvalRubricFamily) -> f64 {
        match family {
            EvalRubricFamily::ResponseQuality => match self {
                Self::Concision => 0.20,
                Self::Clarity => 0.20,
                Self::Groundedness => 0.35,
                Self::Completeness => 0.25,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvalRubricDimensionVerdict {
    pub dimension: EvalRubricDimension,
    pub pass: bool,
    pub score: f64,
    pub confidence: f64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvalRubricHumanLabel {
    pub pass: bool,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvalRubricCalibrationInput {
    pub consensus_pass: bool,
    pub consensus_score: f64,
    pub dimensions: BTreeMap<String, EvalRubricHumanLabel>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum EvalHumanReviewReason {
    LowConfidence,
    NearThreshold,
    CalibrationDisagreement,
}

impl EvalHumanReviewReason {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::LowConfidence => "low_confidence",
            Self::NearThreshold => "near_threshold",
            Self::CalibrationDisagreement => "calibration_disagreement",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvalRubricCalibration {
    pub consensus_pass: bool,
    pub consensus_score: f64,
    pub agreement: bool,
    pub score_delta: f64,
    pub disagreement_dimensions: Vec<EvalRubricDimension>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disagreement_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvalRubricVerdict {
    pub family: EvalRubricFamily,
    pub prompt_version: String,
    pub schema_version: String,
    pub pass: bool,
    pub overall_score: f64,
    pub threshold: f64,
    pub confidence: f64,
    pub rationale: String,
    pub dimensions: Vec<EvalRubricDimensionVerdict>,
    pub requires_human_review: bool,
    pub review_reasons: Vec<EvalHumanReviewReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calibration: Option<EvalRubricCalibration>,
}

pub fn build_rubric_verdict(
    family: EvalRubricFamily,
    prompt_version: &str,
    schema_version: &str,
    mut dimensions: Vec<EvalRubricDimensionVerdict>,
    rationale: impl Into<String>,
    calibration_input: Option<EvalRubricCalibrationInput>,
) -> EvalRubricVerdict {
    dimensions.sort_by_key(|dimension| dimension.dimension);

    let overall_score = weighted_average(family, &dimensions);
    let confidence = dimensions
        .iter()
        .map(|dimension| dimension.confidence)
        .fold(1.0, f64::min);
    let threshold = family.pass_threshold();
    let required_dimensions = EvalRubricDimension::all_for_family(family);
    let dimensions_pass = required_dimensions.iter().all(|required| {
        dimensions
            .iter()
            .find(|dimension| dimension.dimension == *required)
            .map(|dimension| dimension.pass && dimension.score >= required.minimum_score())
            .unwrap_or(false)
    });
    let pass = overall_score >= threshold && dimensions_pass;

    let mut review_reasons = Vec::new();
    if confidence < LOW_CONFIDENCE_THRESHOLD {
        review_reasons.push(EvalHumanReviewReason::LowConfidence);
    }
    if (overall_score - threshold).abs() <= NEAR_THRESHOLD_DELTA {
        review_reasons.push(EvalHumanReviewReason::NearThreshold);
    }

    let calibration = calibration_input.map(|input| {
        let (agreement, score_delta, disagreement_dimensions) =
            calibration_agreement(&dimensions, &input);
        if !agreement {
            review_reasons.push(EvalHumanReviewReason::CalibrationDisagreement);
        }
        let disagreement_reason = if agreement {
            None
        } else {
            Some(format!(
                "model_pass={}, consensus_pass={}, score_delta={:.3}",
                pass, input.consensus_pass, score_delta
            ))
        };
        EvalRubricCalibration {
            consensus_pass: input.consensus_pass,
            consensus_score: input.consensus_score,
            agreement,
            score_delta,
            disagreement_dimensions,
            disagreement_reason,
        }
    });

    review_reasons.sort();
    review_reasons.dedup();

    EvalRubricVerdict {
        family,
        prompt_version: prompt_version.to_string(),
        schema_version: schema_version.to_string(),
        pass,
        overall_score,
        threshold,
        confidence,
        rationale: rationale.into(),
        dimensions,
        requires_human_review: !review_reasons.is_empty(),
        review_reasons,
        calibration,
    }
}

fn weighted_average(family: EvalRubricFamily, dimensions: &[EvalRubricDimensionVerdict]) -> f64 {
    let mut weighted_sum = 0.0;
    let mut total_weight = 0.0;
    for dimension in EvalRubricDimension::all_for_family(family) {
        if let Some(verdict) = dimensions
            .iter()
            .find(|entry| entry.dimension == *dimension)
        {
            let weight = dimension.weight(family);
            weighted_sum += verdict.score * weight;
            total_weight += weight;
        }
    }
    if total_weight == 0.0 {
        0.0
    } else {
        weighted_sum / total_weight
    }
}

fn calibration_agreement(
    dimensions: &[EvalRubricDimensionVerdict],
    input: &EvalRubricCalibrationInput,
) -> (bool, f64, Vec<EvalRubricDimension>) {
    let model_overall = weighted_average(EvalRubricFamily::ResponseQuality, dimensions);
    let score_delta = (model_overall - input.consensus_score).abs();
    let mut disagreement_dimensions = Vec::new();

    for dimension in dimensions {
        let Some(label) = input.dimensions.get(dimension.dimension.as_str()) else {
            disagreement_dimensions.push(dimension.dimension);
            continue;
        };
        if label.pass != dimension.pass
            || (label.score - dimension.score).abs() > CALIBRATION_SCORE_TOLERANCE
        {
            disagreement_dimensions.push(dimension.dimension);
        }
    }

    let model_pass = model_overall >= RUBRIC_PASS_THRESHOLD
        && dimensions.iter().all(|dimension| {
            dimension.pass && dimension.score >= dimension.dimension.minimum_score()
        });
    let agreement = model_pass == input.consensus_pass
        && score_delta <= CALIBRATION_SCORE_TOLERANCE
        && disagreement_dimensions.is_empty();

    (agreement, score_delta, disagreement_dimensions)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{
        EvalHumanReviewReason, EvalRubricCalibrationInput, EvalRubricDimension,
        EvalRubricDimensionVerdict, EvalRubricFamily, EvalRubricHumanLabel, build_rubric_verdict,
    };

    fn dimension(
        dimension: EvalRubricDimension,
        score: f64,
        confidence: f64,
    ) -> EvalRubricDimensionVerdict {
        EvalRubricDimensionVerdict {
            dimension,
            pass: score >= dimension.minimum_score(),
            score,
            confidence,
            reason: format!("{} scored {:.2}", dimension.as_str(), score),
        }
    }

    #[test]
    fn threshold_semantics_are_strict() {
        let verdict = build_rubric_verdict(
            EvalRubricFamily::ResponseQuality,
            "judge-v1",
            "schema-v1",
            vec![
                dimension(EvalRubricDimension::Concision, 0.58, 0.95),
                dimension(EvalRubricDimension::Clarity, 0.76, 0.95),
                dimension(EvalRubricDimension::Groundedness, 0.78, 0.95),
                dimension(EvalRubricDimension::Completeness, 0.76, 0.95),
            ],
            "borderline but below threshold",
            None,
        );

        assert!(!verdict.pass);
        assert!(
            verdict
                .review_reasons
                .contains(&EvalHumanReviewReason::NearThreshold)
        );
    }

    #[test]
    fn low_confidence_routes_to_human_review() {
        let verdict = build_rubric_verdict(
            EvalRubricFamily::ResponseQuality,
            "judge-v1",
            "schema-v1",
            vec![
                dimension(EvalRubricDimension::Concision, 0.90, 0.68),
                dimension(EvalRubricDimension::Clarity, 0.88, 0.92),
                dimension(EvalRubricDimension::Groundedness, 0.91, 0.93),
                dimension(EvalRubricDimension::Completeness, 0.87, 0.94),
            ],
            "good answer but judge confidence is low",
            None,
        );

        assert!(verdict.pass);
        assert!(verdict.requires_human_review);
        assert!(
            verdict
                .review_reasons
                .contains(&EvalHumanReviewReason::LowConfidence)
        );
    }

    #[test]
    fn calibration_disagreement_routes_to_human_review() {
        let mut dimensions = BTreeMap::new();
        dimensions.insert(
            "concision".to_string(),
            EvalRubricHumanLabel {
                pass: false,
                score: 0.40,
            },
        );
        dimensions.insert(
            "clarity".to_string(),
            EvalRubricHumanLabel {
                pass: true,
                score: 0.80,
            },
        );
        dimensions.insert(
            "groundedness".to_string(),
            EvalRubricHumanLabel {
                pass: true,
                score: 0.82,
            },
        );
        dimensions.insert(
            "completeness".to_string(),
            EvalRubricHumanLabel {
                pass: true,
                score: 0.80,
            },
        );

        let verdict = build_rubric_verdict(
            EvalRubricFamily::ResponseQuality,
            "judge-v1",
            "schema-v1",
            vec![
                dimension(EvalRubricDimension::Concision, 0.92, 0.94),
                dimension(EvalRubricDimension::Clarity, 0.85, 0.94),
                dimension(EvalRubricDimension::Groundedness, 0.88, 0.94),
                dimension(EvalRubricDimension::Completeness, 0.84, 0.94),
            ],
            "model liked the answer more than humans did",
            Some(EvalRubricCalibrationInput {
                consensus_pass: false,
                consensus_score: 0.66,
                dimensions,
            }),
        );

        assert!(verdict.requires_human_review);
        assert!(
            verdict
                .review_reasons
                .contains(&EvalHumanReviewReason::CalibrationDisagreement)
        );
        assert_eq!(
            verdict
                .calibration
                .as_ref()
                .map(|calibration| calibration.agreement),
            Some(false)
        );
    }
}
