use anyhow::Result;
use hydra_ai::AiNegotiator;
use crate::models::ContentNode;
use std::sync::Arc;
use tracing::{debug, warn};

/// Uses the local LLM (via hydra-ai's inference engine) to generate
/// hierarchical summaries of text content.
pub struct Summarizer {
    ai: Arc<AiNegotiator>,
    max_tokens: usize,
}

impl Summarizer {
    pub fn new(ai: Arc<AiNegotiator>, max_tokens: usize) -> Self {
        Self { ai, max_tokens }
    }

    /// Process raw text into a hierarchical ContentNode tree with 4 fold levels.
    /// Uses a single LLM call with structured output to generate all levels at once.
    pub async fn process(&self, text: &str) -> Result<ContentNode> {
        let prompt = format!(
            "<|im_start|>system\n\
            You are a content analysis assistant. Given text, produce a structured summary.\n\
            Output ONLY a valid JSON object with these fields:\n\
            - \"headline\": a single sentence capturing the main point (max 15 words)\n\
            - \"summary\": a concise summary in 3-5 sentences\n\
            - \"key_points\": bullet points of key facts and claims, as a single string with newlines\n\
            Do NOT include the original text in your output.\n\
            <|im_end|>\n\
            <|im_start|>user\n{}<|im_end|>\n\
            <|im_start|>assistant\n",
            text
        );

        let mut infer_guard = self.ai.infer().lock().await;
        if let Some(infer) = infer_guard.as_mut() {
            match infer.generate(&prompt, self.max_tokens) {
                Ok(response) => {
                    debug!("Summarizer raw response: {}", response);

                    let json_str = extract_json(&response);
                    if let Ok(parsed) = serde_json::from_str::<SummaryResponse>(json_str) {
                        return Ok(ContentNode::build_tree(
                            parsed.headline,
                            parsed.summary,
                            parsed.key_points,
                            text.to_string(),
                        ));
                    } else {
                        warn!("Failed to parse summarizer JSON, using first-sentence fallback");
                    }
                }
                Err(e) => {
                    warn!("Summarizer inference failed: {}, using fallback", e);
                }
            }
        }
        drop(infer_guard);

        Ok(self.fallback_fold(text))
    }

    /// Simple heuristic folding when LLM is unavailable:
    /// headline = first sentence, summary = first paragraph, key_points = first 3 sentences
    fn fallback_fold(&self, text: &str) -> ContentNode {
        let sentences: Vec<&str> = text
            .split(|c: char| c == '.' || c == '!' || c == '?')
            .filter(|s| !s.trim().is_empty())
            .collect();

        let headline = sentences
            .first()
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| text.chars().take(80).collect::<String>());

        let summary = sentences
            .iter()
            .take(3)
            .map(|s| s.trim())
            .collect::<Vec<_>>()
            .join(". ")
            + ".";

        let key_points = sentences
            .iter()
            .take(5)
            .map(|s| format!("- {}", s.trim()))
            .collect::<Vec<_>>()
            .join("\n");

        ContentNode::build_tree(headline, summary, key_points, text.to_string())
    }
}

#[derive(serde::Deserialize)]
struct SummaryResponse {
    headline: String,
    summary: String,
    key_points: String,
}

/// Extract the first JSON object from a string that may contain surrounding text.
fn extract_json(text: &str) -> &str {
    if let Some(start) = text.find('{') {
        if let Some(end) = text.rfind('}') {
            return &text[start..=end];
        }
    }
    text
}
