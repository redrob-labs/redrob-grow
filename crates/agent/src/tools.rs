//! Tool schemas for the model-backed planner.
//!
//! The model never touches a switch directly. It is offered exactly these tools, each a thin
//! description of a [`grow_hal::Command`] or a question about the observation, and whatever it
//! calls goes through the same guard as the rule planner. The schemas are JSON so they can be
//! handed to any OpenAI-compatible endpoint, including the shared Redrob engine daemon.

use grow_hal::Switch;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

fn switch_names() -> Vec<String> {
    Switch::ALL.iter().map(|s| serde_json::to_value(s).expect("enum").as_str().expect("string").to_owned()).collect()
}

/// The full tool set offered to a planner model for one cycle.
pub fn specs() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            name: "switch_on".into(),
            description: "Turn a switch on for a bounded time. The safety guard may shorten or refuse it. Give one short reason.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "switch": { "type": "string", "enum": switch_names() },
                    "for_secs": { "type": "integer", "minimum": 1, "maximum": 3600 },
                    "reason": { "type": "string", "maxLength": 120 }
                },
                "required": ["switch", "for_secs", "reason"],
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: "switch_off".into(),
            description: "Turn a switch off now. Give one short reason.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "switch": { "type": "string", "enum": switch_names() },
                    "reason": { "type": "string", "maxLength": 120 }
                },
                "required": ["switch", "reason"],
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: "note".into(),
            description: "Record an observation for the human (a trend, a worry, a suggestion to change the recipe). Takes no action.".into(),
            parameters: json!({
                "type": "object",
                "properties": { "text": { "type": "string", "maxLength": 400 } },
                "required": ["text"],
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: "ask_human".into(),
            description: "Stop and ask the operator. Use when a reading makes no physical sense or the crop looks wrong in the photo.".into(),
            parameters: json!({
                "type": "object",
                "properties": { "question": { "type": "string", "maxLength": 400 } },
                "required": ["question"],
                "additionalProperties": false
            }),
        },
    ]
}

/// The planner's output as the model must emit it: a list of tool calls.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "tool", rename_all = "snake_case")]
pub enum ToolCall {
    SwitchOn { switch: Switch, for_secs: u32, reason: String },
    SwitchOff { switch: Switch, reason: String },
    Note { text: String },
    AskHuman { question: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specs_are_valid_json_schema_shaped() {
        let s = specs();
        assert_eq!(s.len(), 4);
        for t in &s {
            assert_eq!(t.parameters["type"], "object");
            assert!(t.parameters["required"].is_array());
        }
        let names = s[0].parameters["properties"]["switch"]["enum"].as_array().unwrap();
        assert_eq!(names.len(), Switch::ALL.len());
        assert!(names.contains(&json!("dose_ph_down")));
    }

    #[test]
    fn tool_calls_round_trip() {
        let calls = vec![
            ToolCall::SwitchOn { switch: Switch::Heater, for_secs: 600, reason: "cold water".into() },
            ToolCall::SwitchOff { switch: Switch::Light, reason: "night".into() },
            ToolCall::Note { text: "EC drifting up".into() },
            ToolCall::AskHuman { question: "leaves yellow at tips?".into() },
        ];
        let json = serde_json::to_string(&calls).unwrap();
        assert!(json.contains(r#""tool":"switch_on""#));
        let back: Vec<ToolCall> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, calls);
    }
}
