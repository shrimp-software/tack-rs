use crate::tools::{ToolDescriptor, ToolRegistry};
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub path: String,
    pub name: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    pub score: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolDescription {
    pub descriptor: ToolDescriptor,
    pub type_signature: String,
}

pub fn search_tools(registry: &ToolRegistry, query: &str, limit: usize) -> Vec<SearchHit> {
    let terms = query
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|term| !term.is_empty())
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>();

    let mut hits = registry
        .iter()
        .filter_map(|tool| score_tool(tool, &terms).map(|score| hit(tool, score)))
        .collect::<Vec<_>>();

    hits.sort_by_key(|hit| (Reverse(hit.score), hit.path.clone()));
    hits.truncate(limit);
    hits
}

pub fn describe_tool(registry: &ToolRegistry, path: &str) -> Option<ToolDescription> {
    registry.by_path(path).map(|descriptor| ToolDescription {
        descriptor: descriptor.clone(),
        type_signature: format!(
            "(input: {}): Promise<ToolCallOutput<{}>>",
            super::type_signature::schema_to_type(descriptor.input_schema()),
            super::type_signature::schema_to_type(descriptor.output_schema())
        ),
    })
}

fn score_tool(tool: &ToolDescriptor, terms: &[String]) -> Option<usize> {
    if terms.is_empty() {
        return Some(1);
    }
    let annotations = tool.annotations();
    let annotation_flags = [
        annotations
            .read_only
            .map(|value| if value { "read_only" } else { "write_capable" }),
        annotations
            .destructive
            .map(|value| if value { "destructive" } else { "safe" }),
        annotations.idempotent.map(|value| {
            if value {
                "idempotent"
            } else {
                "non_idempotent"
            }
        }),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    let text = [
        tool.path(),
        tool.name(),
        tool.title().unwrap_or_default(),
        tool.description().unwrap_or_default(),
        annotations.capability.as_deref().unwrap_or_default(),
        annotations.version.as_deref().unwrap_or_default(),
        &tool.tags().join(" "),
        &annotation_flags.join(" "),
    ]
    .join(" ")
    .to_ascii_lowercase();
    let score = terms
        .iter()
        .map(|term| {
            if tool.path().eq_ignore_ascii_case(term) || tool.name().eq_ignore_ascii_case(term) {
                10
            } else if text.contains(term) {
                3
            } else {
                0
            }
        })
        .sum();
    (score > 0).then_some(score)
}

fn hit(tool: &ToolDescriptor, score: usize) -> SearchHit {
    SearchHit {
        path: tool.path().to_string(),
        name: tool.name().to_string(),
        title: tool.title().map(str::to_string),
        description: tool.description().map(str::to_string),
        score,
    }
}
