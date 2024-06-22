use std::{cmp::Ordering, error::Error};

use crate::result::ShaderResult;

#[derive(Debug)]
struct SpanInString {
    start: usize,
    end: usize,
    token_span: proc_macro::Span,
}

/// Returns true only if the given character cannot be in an identifier. Returning false gives no information.
fn non_identifier_char(c: char) -> bool {
    matches!(
        c,
        '(' | ')'
            | '{'
            | '}'
            | '['
            | ']'
            | '<'
            | '>'
            | ','
            | '+'
            | '*'
            | '/'
            | '!'
            | '\\'
            | '"'
            | '\''
            | '|'
            | '='
            | '^'
            | '&'
            | ';'
            | ':'
            | '?'
            | '%'
            | '@'
            | '#'
            | '~'
            | '.'
            | '£'
            | '$'
            | '`'
    )
}

/// Returns true if the two tokens should be separated by a space within wgsl source code.
fn should_add_space_between(last: char, next: char) -> bool {
    if last == '-' && next == '>' {
        return false; // Might be a function return like `->`
    }
    if non_identifier_char(last) && next == '=' {
        return false; // Might be a comparison like `>=`, `!=` or `==`
    }
    if last == next && non_identifier_char(next) {
        return false; // Might be a double operator like `++`
    }
    if last == ':' || next == ':' {
        return false; // Might be an import path like `a::b`
    }
    true
}

/// Shader sourcecode generated from the token stream provided
pub(crate) struct Sourcecode {
    src: String,
    spans: Vec<SpanInString>,
    errors: Vec<(String, Vec<proc_macro::Span>)>,
}

impl Sourcecode {
    pub(crate) fn new() -> Self {
        Self {
            src: String::new(),
            spans: Vec::new(),
            errors: Vec::new(),
        }
    }

    /// Adds a token to a string and records its span both in the source code, and the resulting string.
    fn push_token(&mut self, token: &str, span: proc_macro::Span) {
        let next_start_char = match token.chars().next() {
            Some(s) => s,
            None => return,
        };

        let start = self.src.len();
        if self
            .src
            .ends_with(move |last_char| should_add_space_between(last_char, next_start_char))
        {
            self.src += " ";
        }
        self.src += token;
        let end = self.src.len();

        self.spans.push(SpanInString {
            start,
            end,
            token_span: span,
        })
    }

    /// Converts a sequence of tokens to a string, tracking the spans of the tokens that constitute the string.
    pub(crate) fn append_tokens(&mut self, tokens: proc_macro::TokenStream) {
        for token in tokens {
            match token {
                proc_macro::TokenTree::Group(g) => {
                    let delims = match g.delimiter() {
                        proc_macro::Delimiter::Parenthesis => Some(("(", ")")),
                        proc_macro::Delimiter::Brace => Some(("{", "}")),
                        proc_macro::Delimiter::Bracket => Some(("[", "]")),
                        proc_macro::Delimiter::None => None,
                    };

                    if let Some((start, _)) = delims {
                        self.push_token(start, g.span_open());
                    }

                    self.append_tokens(g.stream());

                    if let Some((_, end)) = delims {
                        self.push_token(end, g.span_close());
                    }
                }
                _ => self.push_token(&token.to_string(), token.span()),
            }
        }
    }

    // Finds all spans associated with a range of characters
    fn get_spans_within(&self, start: usize, end: usize) -> Vec<proc_macro::Span> {
        let span_start = self.spans.binary_search_by(move |span| {
            assert!(span.start <= span.end);

            if start >= span.start && start < span.end {
                Ordering::Equal
            } else if start < span.start {
                Ordering::Greater
            } else {
                Ordering::Less
            }
        });
        let span_start = match span_start {
            Ok(s) => s,
            Err(s) => s.saturating_sub(1),
        };

        let span_end = self.spans.binary_search_by(move |span| {
            assert!(span.start <= span.end);

            if end > span.start && end <= span.end {
                Ordering::Equal
            } else if end <= span.start {
                Ordering::Greater
            } else {
                Ordering::Less
            }
        });
        let span_end = match span_end {
            Ok(s) => usize::min(s + 1, self.spans.len()),
            Err(s) => s,
        };

        self.spans[span_start..span_end]
            .iter()
            .map(|span| span.token_span)
            .collect()
    }

    pub(crate) fn push_naga_error(&mut self, loc: naga::Span, msg: String) {
        let error_spans = if let Some(loc) = loc.to_range() {
            self.get_spans_within(loc.start, loc.end)
        } else {
            self.spans.iter().map(|s| s.token_span).collect()
        };

        self.errors.push((msg, error_spans))
    }

    fn parse(&mut self) -> Option<naga::Module> {
        match naga::front::wgsl::parse_str(&self.src) {
            Ok(module) => Some(module),
            Err(e) => {
                let mut e_base: &dyn Error = &e;
                let mut message = format!("{}", e);
                while let Some(e) = e_base.source() {
                    message = format!("{}: {}", message, e);
                    e_base = e;
                }

                let labels = e.labels();
                if labels.len() == 0 {
                    self.push_naga_error(naga::Span::new(0, u32::MAX), message.clone());
                } else {
                    for (loc, label) in labels {
                        self.push_naga_error(loc, format!("at {}: {}", label, message));
                    }
                }

                None
            }
        }
    }

    pub(crate) fn complete(mut self) -> ShaderResult {
        let module = self.parse().unwrap_or_default();

        ShaderResult::new(self, module)
    }

    pub(crate) fn errors(&self) -> impl Iterator<Item = &(String, Vec<proc_macro::Span>)> {
        self.errors.iter()
    }
}
