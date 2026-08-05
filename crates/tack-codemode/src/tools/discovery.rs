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
        .tools
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
            super::schema::schema_to_type(&descriptor.input_schema),
            super::schema::schema_to_type(&descriptor.output_schema)
        ),
    })
}

fn score_tool(tool: &ToolDescriptor, terms: &[String]) -> Option<usize> {
    if terms.is_empty() {
        return Some(1);
    }
    let text = [
        tool.path.as_str(),
        tool.name.as_str(),
        tool.title.as_deref().unwrap_or_default(),
        tool.description.as_deref().unwrap_or_default(),
    ]
    .join(" ")
    .to_ascii_lowercase();
    let score = terms
        .iter()
        .map(|term| {
            if tool.path.eq_ignore_ascii_case(term) || tool.name.eq_ignore_ascii_case(term) {
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
        path: tool.path.clone(),
        name: tool.name.clone(),
        title: tool.title.clone(),
        description: tool.description.clone(),
        score,
    }
}
