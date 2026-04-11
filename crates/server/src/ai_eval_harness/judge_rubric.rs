use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::judge::parse_model_json_object;
use super::judge_metrics::{EvalRubricDimension, EvalRubricDimensionVerdict, EvalRubricFamily};

pub const RUBRIC_PROMPT_VERSION: &str = "rustyfin_ai_rubric_phase2_v1";
pub const RUBRIC_RESPONSE_SCHEMA_VERSION: &str = "rustyfin_ai_rubric_response_v1";
pub const PAIRWISE_PROMPT_VERSION: &str = "rustyfin_ai_pairwise_phase3_v1";
pub const PAIRWISE_RESPONSE_SCHEMA_VERSION: &str = "rustyfin_ai_pairwise_response_v1";

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct RubricEvidenceChunk {
    pub id: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedRubricResponse {
    pub rationale: String,
    pub dimensions: Vec<EvalRubricDimensionVerdict>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvalPairwiseSelection {
    A,
    B,
    Tie,
    NoWinner,
}

impl EvalPairwiseSelection {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::A => "a",
            Self::B => "b",
            Self::Tie => "tie",
            Self::NoWinner => "no_winner",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedPairwiseResponse {
    pub selection: EvalPairwiseSelection,
    pub confidence: f64,
    pub rationale: String,
}

#[derive(Debug, Clone, Deserialize)]
struct RawRubricDimension {
    dimension: EvalRubricDimension,
    pass: bool,
    score: f64,
    confidence: f64,
    reason: String,
}

#[derive(Debug, Clone, Deserialize)]
struct RawRubricResponse {
    dimensions: Vec<RawRubricDimension>,
    overall_reason: String,
}

#[derive(Debug, Clone, Deserialize)]
struct RawPairwiseResponse {
    winner: EvalPairwiseSelection,
    confidence: f64,
    reason: String,
}

pub fn build_rubric_prompt(
    family: EvalRubricFamily,
    user_prompt: &str,
    assistant_answer: &str,
    reference_answer: &str,
    evidence_chunks: &[RubricEvidenceChunk],
) -> String {
    let evidence = if evidence_chunks.is_empty() {
        "No grounding evidence was supplied.".to_string()
    } else {
        evidence_chunks
            .iter()
            .map(|chunk| format!("- {}: {}", chunk.id, chunk.text))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let dimensions = EvalRubricDimension::all_for_family(family)
        .iter()
        .map(EvalRubricDimension::as_str)
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "Judge family: {}\n\
         Prompt version: {}\n\
         Response schema version: {}\n\
         Required dimensions: {}\n\
         Score each dimension from 0.0 to 1.0, set pass to true only when the answer meets that dimension, and keep reasons brief and audit-friendly.\n\
         Return only JSON with a `dimensions` array and `overall_reason` string.\n\n\
         User prompt:\n{}\n\n\
         Assistant answer:\n{}\n\n\
         Reference answer:\n{}\n\n\
         Grounding evidence:\n{}",
        family.as_str(),
        RUBRIC_PROMPT_VERSION,
        RUBRIC_RESPONSE_SCHEMA_VERSION,
        dimensions,
        user_prompt,
        assistant_answer,
        reference_answer,
        evidence
    )
}

pub fn rubric_response_schema_json() -> Value {
    json!({
        "version": RUBRIC_RESPONSE_SCHEMA_VERSION,
        "type": "object",
        "required": ["dimensions", "overall_reason"],
        "properties": {
            "dimensions": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["dimension", "pass", "score", "confidence", "reason"],
                    "properties": {
                        "dimension": {
                            "type": "string",
                            "enum": ["concision", "clarity", "groundedness", "completeness"]
                        },
                        "pass": { "type": "boolean" },
                        "score": { "type": "number" },
                        "confidence": { "type": "number" },
                        "reason": { "type": "string" }
                    }
                }
            },
            "overall_reason": { "type": "string" }
        }
    })
}

pub fn build_pairwise_prompt(
    family: EvalRubricFamily,
    user_prompt: &str,
    answer_a: &str,
    answer_b: &str,
    reference_answer: &str,
    evidence_chunks: &[RubricEvidenceChunk],
) -> String {
    let evidence = if evidence_chunks.is_empty() {
        "No grounding evidence was supplied.".to_string()
    } else {
        evidence_chunks
            .iter()
            .map(|chunk| format!("- {}: {}", chunk.id, chunk.text))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let dimensions = EvalRubricDimension::all_for_family(family)
        .iter()
        .map(EvalRubricDimension::as_str)
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "Pairwise judge family: {}\n\
         Prompt version: {}\n\
         Response schema version: {}\n\
         Judge answers A and B using these dimensions: {}\n\
         Pick `winner` as `a`, `b`, `tie`, or `no_winner`. Use `no_winner` when the ordering is unstable or the evidence is insufficient.\n\
         Return only JSON with `winner`, `confidence`, and `reason`.\n\n\
         User prompt:\n{}\n\n\
         Answer A:\n{}\n\n\
         Answer B:\n{}\n\n\
         Reference answer:\n{}\n\n\
         Grounding evidence:\n{}",
        family.as_str(),
        PAIRWISE_PROMPT_VERSION,
        PAIRWISE_RESPONSE_SCHEMA_VERSION,
        dimensions,
        user_prompt,
        answer_a,
        answer_b,
        reference_answer,
        evidence
    )
}

pub fn pairwise_response_schema_json() -> Value {
    json!({
        "version": PAIRWISE_RESPONSE_SCHEMA_VERSION,
        "type": "object",
        "required": ["winner", "confidence", "reason"],
        "properties": {
            "winner": {
                "type": "string",
                "enum": ["a", "b", "tie", "no_winner"]
            },
            "confidence": { "type": "number" },
            "reason": { "type": "string" }
        }
    })
}

pub fn parse_rubric_response(
    raw: &str,
    family: EvalRubricFamily,
) -> Result<ParsedRubricResponse, String> {
    let value = parse_model_json_object(raw)?;
    let response: RawRubricResponse = serde_json::from_value(value)
        .map_err(|error| format!("invalid rubric response: {error}"))?;
    if response.overall_reason.trim().is_empty() {
        return Err("overall_reason must not be blank".to_string());
    }

    let mut dimensions = response
        .dimensions
        .into_iter()
        .map(|dimension| {
            if !(0.0..=1.0).contains(&dimension.score) {
                return Err(format!(
                    "score for {} must be between 0.0 and 1.0",
                    dimension.dimension.as_str()
                ));
            }
            if !(0.0..=1.0).contains(&dimension.confidence) {
                return Err(format!(
                    "confidence for {} must be between 0.0 and 1.0",
                    dimension.dimension.as_str()
                ));
            }
            if dimension.reason.trim().is_empty() {
                return Err(format!(
                    "reason for {} must not be blank",
                    dimension.dimension.as_str()
                ));
            }
            Ok(EvalRubricDimensionVerdict {
                dimension: dimension.dimension,
                pass: dimension.pass,
                score: dimension.score,
                confidence: dimension.confidence,
                reason: dimension.reason,
            })
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;

    dimensions.sort_by_key(|dimension| dimension.dimension);
    for window in dimensions.windows(2) {
        if window[0].dimension == window[1].dimension {
            return Err(format!(
                "rubric response contains duplicate dimension {}",
                window[0].dimension.as_str()
            ));
        }
    }
    if dimensions.len() != EvalRubricDimension::all_for_family(family).len() {
        return Err("rubric response is missing one or more required dimensions".to_string());
    }
    for expected in EvalRubricDimension::all_for_family(family) {
        if !dimensions
            .iter()
            .any(|dimension| dimension.dimension == *expected)
        {
            return Err(format!(
                "rubric response is missing required dimension {}",
                expected.as_str()
            ));
        }
    }

    Ok(ParsedRubricResponse {
        rationale: response.overall_reason,
        dimensions,
    })
}

pub fn parse_pairwise_response(raw: &str) -> Result<ParsedPairwiseResponse, String> {
    let value = parse_model_json_object(raw)?;
    let response: RawPairwiseResponse = serde_json::from_value(value)
        .map_err(|error| format!("invalid pairwise response: {error}"))?;
    if !(0.0..=1.0).contains(&response.confidence) {
        return Err("pairwise confidence must be between 0.0 and 1.0".to_string());
    }
    if response.reason.trim().is_empty() {
        return Err("pairwise reason must not be blank".to_string());
    }

    Ok(ParsedPairwiseResponse {
        selection: response.winner,
        confidence: response.confidence,
        rationale: response.reason,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        PAIRWISE_PROMPT_VERSION, PAIRWISE_RESPONSE_SCHEMA_VERSION, ParsedPairwiseResponse,
        ParsedRubricResponse, RUBRIC_PROMPT_VERSION, RUBRIC_RESPONSE_SCHEMA_VERSION,
        build_pairwise_prompt, build_rubric_prompt, pairwise_response_schema_json,
        parse_pairwise_response, parse_rubric_response, rubric_response_schema_json,
    };
    use crate::ai_eval_harness::judge_metrics::{EvalRubricDimension, EvalRubricFamily};

    #[test]
    fn parser_accepts_valid_rubric_payload() {
        let payload = r#"{
          "dimensions": [
            {"dimension":"concision","pass":true,"score":0.90,"confidence":0.95,"reason":"brief"},
            {"dimension":"clarity","pass":true,"score":0.88,"confidence":0.94,"reason":"clear"},
            {"dimension":"groundedness","pass":true,"score":0.93,"confidence":0.97,"reason":"grounded"},
            {"dimension":"completeness","pass":true,"score":0.87,"confidence":0.92,"reason":"complete"}
          ],
          "overall_reason":"The answer is concise and grounded."
        }"#;
        let parsed = parse_rubric_response(payload, EvalRubricFamily::ResponseQuality).unwrap();
        assert_eq!(
            parsed,
            ParsedRubricResponse {
                rationale: "The answer is concise and grounded.".to_string(),
                dimensions: vec![
                    EvalRubricDimension::Concision,
                    EvalRubricDimension::Clarity,
                    EvalRubricDimension::Groundedness,
                    EvalRubricDimension::Completeness,
                ]
                .into_iter()
                .map(|dimension| {
                    crate::ai_eval_harness::judge_metrics::EvalRubricDimensionVerdict {
                        dimension,
                        pass: true,
                        score: match dimension {
                            EvalRubricDimension::Concision => 0.90,
                            EvalRubricDimension::Clarity => 0.88,
                            EvalRubricDimension::Groundedness => 0.93,
                            EvalRubricDimension::Completeness => 0.87,
                        },
                        confidence: match dimension {
                            EvalRubricDimension::Concision => 0.95,
                            EvalRubricDimension::Clarity => 0.94,
                            EvalRubricDimension::Groundedness => 0.97,
                            EvalRubricDimension::Completeness => 0.92,
                        },
                        reason: match dimension {
                            EvalRubricDimension::Concision => "brief".to_string(),
                            EvalRubricDimension::Clarity => "clear".to_string(),
                            EvalRubricDimension::Groundedness => "grounded".to_string(),
                            EvalRubricDimension::Completeness => "complete".to_string(),
                        },
                    }
                })
                .collect(),
            }
        );
    }

    #[test]
    fn parser_rejects_missing_dimensions() {
        let payload = r#"{
          "dimensions": [
            {"dimension":"concision","pass":true,"score":0.90,"confidence":0.95,"reason":"brief"}
          ],
          "overall_reason":"Too incomplete."
        }"#;
        let error = parse_rubric_response(payload, EvalRubricFamily::ResponseQuality).unwrap_err();
        assert!(error.contains("missing one or more required dimensions"));
    }

    #[test]
    fn prompt_and_schema_versions_are_embedded() {
        let prompt = build_rubric_prompt(
            EvalRubricFamily::ResponseQuality,
            "What is my next event?",
            "Your next event is your dentist appointment at 3 PM.",
            "The next event is a dentist appointment at 3 PM.",
            &[],
        );
        assert!(prompt.contains(RUBRIC_PROMPT_VERSION));
        assert!(prompt.contains(RUBRIC_RESPONSE_SCHEMA_VERSION));
        assert_eq!(
            rubric_response_schema_json()["version"].as_str(),
            Some(RUBRIC_RESPONSE_SCHEMA_VERSION)
        );

        let pairwise_prompt = build_pairwise_prompt(
            EvalRubricFamily::ResponseQuality,
            "What is my next event?",
            "Answer A",
            "Answer B",
            "The next event is a dentist appointment at 3 PM.",
            &[],
        );
        assert!(pairwise_prompt.contains(PAIRWISE_PROMPT_VERSION));
        assert_eq!(
            pairwise_response_schema_json()["version"].as_str(),
            Some(PAIRWISE_RESPONSE_SCHEMA_VERSION)
        );
    }

    #[test]
    fn parser_accepts_valid_pairwise_payload() {
        let payload = r#"{
          "winner": "b",
          "confidence": 0.91,
          "reason": "B is more direct without losing any grounded detail."
        }"#;
        let parsed = parse_pairwise_response(payload).unwrap();
        assert_eq!(
            parsed,
            ParsedPairwiseResponse {
                selection: super::EvalPairwiseSelection::B,
                confidence: 0.91,
                rationale: "B is more direct without losing any grounded detail.".to_string(),
            }
        );
    }

    #[test]
    fn parser_rejects_blank_pairwise_reason() {
        let payload = r#"{
          "winner": "a",
          "confidence": 0.80,
          "reason": "  "
        }"#;
        let error = parse_pairwise_response(payload).unwrap_err();
        assert!(error.contains("must not be blank"));
    }
}
