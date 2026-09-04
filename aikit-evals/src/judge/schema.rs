//! The reply envelope (spec eval-judge R6), the rubric text (R5) and the
//! scoring the engine does from a validated reply (R5).

use super::config::{Criterion, CriterionKind, OVERALL};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// JSON schema the judge's reply must satisfy: every declared criterion
/// exactly once by name, `answer` an integer in `[1, scale]` or a boolean,
/// `reasoning` before `answer` so the model commits to a rationale first.
pub fn reply_schema(criteria: &[Criterion]) -> Value {
    let items: Vec<Value> = criteria
        .iter()
        .map(|c| {
            let answer = match c.kind {
                CriterionKind::Scale => json!({
                    "type": "integer",
                    "minimum": 1,
                    "maximum": c.scale,
                }),
                CriterionKind::Bool => json!({ "type": "boolean" }),
            };
            json!({
                "type": "object",
                "properties": {
                    "name": { "const": c.name },
                    "reasoning": { "type": "string" },
                    "answer": answer,
                },
                "required": ["name", "reasoning", "answer"],
                "additionalProperties": false,
            })
        })
        .collect();
    let each_once: Vec<Value> = criteria
        .iter()
        .map(|c| {
            json!({
                "contains": {
                    "type": "object",
                    "properties": { "name": { "const": c.name } },
                    "required": ["name"],
                }
            })
        })
        .collect();
    json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "type": "object",
        "properties": {
            "criteria": {
                "type": "array",
                "minItems": criteria.len(),
                "maxItems": criteria.len(),
                "items": { "oneOf": items },
                "allOf": each_once,
            },
            "notes": { "type": "string" },
        },
        "required": ["criteria"],
        "additionalProperties": false,
    })
}

/// What `{{output_contract}}` renders to: the schema, pretty-printed.
pub fn output_contract(criteria: &[Criterion]) -> String {
    serde_json::to_string_pretty(&reply_schema(criteria)).unwrap_or_default()
}

/// What `{{rubric}}` renders to: one block per criterion — name, range,
/// description — separated by blank lines. Plain text, no markup.
pub fn rubric_text(criteria: &[Criterion]) -> String {
    criteria
        .iter()
        .map(|c| {
            let range = match c.kind {
                CriterionKind::Scale => format!("1–{}", c.scale),
                CriterionKind::Bool => "yes / no".to_string(),
            };
            format!("{} ({})\n{}", c.name, range, c.description.trim())
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Score a reply that already passed [`reply_schema`]: scale `a` on `k`
/// maps to `(a − 1) / (k − 1)`, bool to 1.0 / 0.0, `overall` is the
/// unweighted mean. Errors only on a reply the schema would have rejected.
pub fn score_reply(criteria: &[Criterion], reply: &Value) -> Result<BTreeMap<String, f64>, String> {
    let entries = reply
        .get("criteria")
        .and_then(Value::as_array)
        .ok_or_else(|| "reply has no `criteria` array".to_string())?;
    let mut scores = BTreeMap::new();
    for c in criteria {
        let matching: Vec<&Value> = entries
            .iter()
            .filter(|e| e.get("name").and_then(Value::as_str) == Some(c.name.as_str()))
            .collect();
        let entry = match matching.as_slice() {
            [one] => *one,
            [] => return Err(format!("criterion '{}' is missing from the reply", c.name)),
            _ => return Err(format!("criterion '{}' appears more than once", c.name)),
        };
        let answer = entry
            .get("answer")
            .ok_or_else(|| format!("criterion '{}' has no `answer`", c.name))?;
        let score = match c.kind {
            CriterionKind::Scale => {
                let a = answer
                    .as_i64()
                    .ok_or_else(|| format!("criterion '{}': answer is not an integer", c.name))?;
                let k = i64::from(c.scale);
                if a < 1 || a > k {
                    return Err(format!(
                        "criterion '{}': answer {} is outside 1..={}",
                        c.name, a, k
                    ));
                }
                (a - 1) as f64 / (k - 1) as f64
            }
            CriterionKind::Bool => {
                let b = answer
                    .as_bool()
                    .ok_or_else(|| format!("criterion '{}': answer is not a boolean", c.name))?;
                if b {
                    1.0
                } else {
                    0.0
                }
            }
        };
        scores.insert(c.name.clone(), score);
    }
    if scores.is_empty() {
        return Err("no criteria to score".to_string());
    }
    let overall = scores.values().sum::<f64>() / scores.len() as f64;
    scores.insert(OVERALL.to_string(), overall);
    Ok(scores)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn criteria() -> Vec<Criterion> {
        vec![
            Criterion {
                name: "clear".into(),
                kind: CriterionKind::Scale,
                scale: 5,
                description: "Is it clear?".into(),
            },
            Criterion {
                name: "runs".into(),
                kind: CriterionKind::Bool,
                scale: 2,
                description: "Would it run?".into(),
            },
        ]
    }

    fn validate(reply: &Value) -> Result<(), String> {
        let schema = reply_schema(&criteria());
        let v = jsonschema::validator_for(&schema).map_err(|e| e.to_string())?;
        let errors: Vec<String> = v.iter_errors(reply).map(|e| e.to_string()).collect();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }

    #[test]
    fn well_formed_reply_validates_and_scores() {
        let reply = json!({
            "criteria": [
                {"name": "clear", "reasoning": "ok", "answer": 4},
                {"name": "runs", "reasoning": "yes", "answer": true}
            ],
            "notes": "fine"
        });
        validate(&reply).expect("valid");
        let scores = score_reply(&criteria(), &reply).unwrap();
        assert_eq!(scores["clear"], 0.75);
        assert_eq!(scores["runs"], 1.0);
        assert_eq!(scores["overall"], 0.875);
    }

    #[test]
    fn schema_rejects_missing_duplicate_out_of_range_and_extra() {
        let missing = json!({"criteria": [
            {"name": "clear", "reasoning": "ok", "answer": 4}
        ]});
        assert!(validate(&missing).is_err());
        let duplicate = json!({"criteria": [
            {"name": "clear", "reasoning": "ok", "answer": 4},
            {"name": "clear", "reasoning": "ok", "answer": 2}
        ]});
        assert!(validate(&duplicate).is_err());
        let out_of_range = json!({"criteria": [
            {"name": "clear", "reasoning": "ok", "answer": 6},
            {"name": "runs", "reasoning": "yes", "answer": true}
        ]});
        assert!(validate(&out_of_range).is_err());
        let wrong_type = json!({"criteria": [
            {"name": "clear", "reasoning": "ok", "answer": "4"},
            {"name": "runs", "reasoning": "yes", "answer": true}
        ]});
        assert!(validate(&wrong_type).is_err());
        let extra_field = json!({"criteria": [
            {"name": "clear", "reasoning": "ok", "answer": 4, "weight": 1},
            {"name": "runs", "reasoning": "yes", "answer": true}
        ]});
        assert!(validate(&extra_field).is_err());
        let extra_top = json!({"criteria": [
            {"name": "clear", "reasoning": "ok", "answer": 4},
            {"name": "runs", "reasoning": "yes", "answer": true}
        ], "score": 1});
        assert!(validate(&extra_top).is_err());
        let no_reasoning = json!({"criteria": [
            {"name": "clear", "answer": 4},
            {"name": "runs", "reasoning": "yes", "answer": true}
        ]});
        assert!(validate(&no_reasoning).is_err());
    }

    #[test]
    fn scale_maps_endpoints_to_zero_and_one() {
        let c = vec![Criterion {
            name: "c".into(),
            kind: CriterionKind::Scale,
            scale: 3,
            description: String::new(),
        }];
        for (answer, expect) in [(1, 0.0), (2, 0.5), (3, 1.0)] {
            let reply = json!({"criteria": [{"name": "c", "reasoning": "", "answer": answer}]});
            assert_eq!(score_reply(&c, &reply).unwrap()["c"], expect);
        }
    }

    #[test]
    fn score_reply_refuses_what_the_schema_would() {
        let reply = json!({"criteria": [{"name": "clear", "reasoning": "", "answer": 9}]});
        assert!(score_reply(&criteria(), &reply).is_err());
        assert!(score_reply(&criteria(), &json!({})).is_err());
    }

    #[test]
    fn rubric_text_names_range_and_description() {
        let text = rubric_text(&criteria());
        assert_eq!(
            text,
            "clear (1–5)\nIs it clear?\n\nruns (yes / no)\nWould it run?"
        );
    }

    #[test]
    fn output_contract_is_the_schema() {
        let text = output_contract(&criteria());
        let parsed: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed, reply_schema(&criteria()));
        assert!(text.contains("\"maximum\": 5"));
    }
}
