use crate::{ContentDelta, ThinkingDelta};

const REDACTED_OPEN: &str = "<think>";
const REDACTED_CLOSE: &str = "</think>";

#[derive(Debug, Default)]
pub struct ThinkingTagNormalizer {
    inside_thinking: bool,
    carry: String,
}

impl ThinkingTagNormalizer {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, chunk: String) -> Vec<ContentDelta> {
        if chunk.is_empty() {
            return Vec::new();
        }

        let mut input = self.carry.clone();
        input.push_str(&chunk);
        self.carry.clear();

        let mut output = Vec::new();
        let mut cursor = 0;

        while cursor < input.len() {
            if self.inside_thinking {
                if let Some(close_index) = input[cursor..].find(REDACTED_CLOSE) {
                    let absolute = cursor + close_index;
                    let thinking_text = input[cursor..absolute].to_owned();
                    if !thinking_text.is_empty() {
                        output.push(thinking_delta(thinking_text));
                    }
                    cursor = absolute + REDACTED_CLOSE.len();
                    self.inside_thinking = false;
                    continue;
                }

                let thinking_text = input[cursor..].to_owned();
                if !thinking_text.is_empty() {
                    output.push(thinking_delta(thinking_text));
                }
                return output;
            }

            if let Some(open_index) = input[cursor..].find(REDACTED_OPEN) {
                let absolute = cursor + open_index;
                let answer_text = input[cursor..absolute].to_owned();
                if !answer_text.is_empty() {
                    output.push(ContentDelta::Text(answer_text));
                }
                cursor = absolute + REDACTED_OPEN.len();
                self.inside_thinking = true;
                continue;
            }

            if let Some(partial_open) = find_partial_suffix(&input[cursor..], REDACTED_OPEN) {
                let safe_prefix = &input[cursor..input.len() - partial_open.len()];
                if !safe_prefix.is_empty() {
                    output.push(ContentDelta::Text(safe_prefix.to_owned()));
                }
                self.carry = partial_open.to_owned();
                return output;
            }

            output.push(ContentDelta::Text(input[cursor..].to_owned()));
            return output;
        }

        output
    }
}

fn find_partial_suffix<'a>(input: &'a str, tag: &str) -> Option<&'a str> {
    for len in (1..tag.len()).rev() {
        if input.ends_with(&tag[..len]) {
            return Some(&input[input.len() - len..]);
        }
    }
    None
}

fn thinking_delta(text: String) -> ContentDelta {
    ContentDelta::Thinking(ThinkingDelta {
        text: Some(text),
        provider_native: None,
        signature: None,
    })
}

#[cfg(test)]
mod tests {
    use super::ThinkingTagNormalizer;

    #[test]
    fn splits_redacted_thinking_tags_into_thinking_deltas() {
        let mut normalizer = ThinkingTagNormalizer::new();
        let first = normalizer.push("<think>plan".to_owned());
        let second = normalizer.push("</think>Answer".to_owned());

        assert!(matches!(
            first.as_slice(),
            [crate::ContentDelta::Thinking(thinking)] if thinking.text.as_deref() == Some("plan")
        ));
        assert!(matches!(
            second.as_slice(),
            [crate::ContentDelta::Text(answer)] if answer == "Answer"
        ));
    }

    #[test]
    fn keeps_answer_text_outside_thinking_tags() {
        let mut normalizer = ThinkingTagNormalizer::new();
        let chunks = normalizer.push("Prefix <think>hidden</think> Suffix".to_owned());

        assert!(matches!(
            chunks.as_slice(),
            [
                crate::ContentDelta::Text(prefix),
                crate::ContentDelta::Thinking(thinking),
                crate::ContentDelta::Text(suffix),
            ] if prefix == "Prefix " && thinking.text.as_deref() == Some("hidden") && suffix == " Suffix"
        ));
    }
}
