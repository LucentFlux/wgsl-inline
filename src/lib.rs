#![doc = include_str!("../README.md")]

use std::{cmp::Ordering, error::Error};

use quote::ToTokens;

#[derive(Debug)]
struct SpanInString {
    start: usize,
    end: usize,
    token_span: proc_macro::Span,
}

/// Map expressions of the form `#include ""` or `#include foo::bar::BAZ` into a proper stream.
fn process_includes(
    ts: impl Iterator<Item = proc_macro::TokenTree>,
) -> impl Iterator<Item = proc_macro::TokenTree> {
    ts
}

/// Returns true only if the given character cannot be in an identifier. Returning false gives no information.
fn non_identifier_char(c: char) -> bool {
    match c {
        '(' | ')' | '{' | '}' | '[' | ']' | '<' | '>' | ',' | '+' | '*' | '/' | '!' | '\\'
        | '"' | '\'' | '|' | '=' | '^' | '&' | ';' | ':' | '?' | '%' | '@' | '#' | '~' | '.'
        | '£' | '$' | '`' => true,
        _ => false,
    }
}

/// Returns true if the two tokens should be separated by a space within wgsl source code.
fn should_add_space_between(last: char, next: char) -> bool {
    if non_identifier_char(last) {
        return false;
    }
    if non_identifier_char(next) {
        return false;
    }
    return true;
}

/// Shader sourcecode generated from the token stream provided
struct Sourcecode {
    src: String,
    spans: Vec<SpanInString>,
    errors: Vec<(String, Vec<proc_macro::Span>)>,
}

impl Sourcecode {
    fn new() -> Self {
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
    fn append_tokens(&mut self, tokens: proc_macro::TokenStream) {
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
            .map(|span| span.token_span.clone())
            .collect()
    }

    fn push_naga_error(&mut self, loc: naga::Span, msg: String) {
        let error_spans = if let Some(loc) = loc.to_range() {
            self.get_spans_within(loc.start, loc.end)
        } else {
            self.spans.iter().map(|s| s.token_span.clone()).collect()
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

                for (loc, _) in e.labels() {
                    self.push_naga_error(loc, message.clone());
                }

                None
            }
        }
    }

    fn complete(mut self) -> ShaderResult {
        let module = self.parse().unwrap_or(naga::Module::default());

        ShaderResult {
            source: self,
            module,
        }
    }
}

/// The output of the transformations provided by this crate.
struct ShaderResult {
    source: Sourcecode,
    module: naga::Module,
}

impl ShaderResult {
    fn validate(&mut self) {
        let mut validator = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        );
        if let Err(e) = validator.validate(&self.module) {
            let mut e_base: &dyn Error = e.as_inner();
            let mut message = format!("{}", e);
            while let Some(e) = e_base.source() {
                message = format!("{}: {}", message, e);
                e_base = e;
            }

            for (loc, extra) in e.spans() {
                self.source
                    .push_naga_error(loc.clone(), format!("{}: {}", message, extra))
            }
        }
    }

    #[cfg(feature = "minify")]
    fn minify(&mut self) {
        wgsl_minifier::remove_identifiers(&mut self.module);

        let mut validator = naga::valid::Validator::new(
            naga::valid::ValidationFlags::empty(),
            naga::valid::Capabilities::all(),
        );
        if let Some(info) = validator.validate(&self.module).ok() {
            if let Some(src) = naga::back::wgsl::write_string(
                &self.module,
                &info,
                naga::back::wgsl::WriterFlags::empty(),
            )
            .ok()
            {
                self.source.src = wgsl_minifier::minify_wgsl_source_whitespace(&src)
            }
        }
    }

    fn to_items(self) -> Vec<syn::Item> {
        let mut items = Vec::new();

        let src = self.source.src;
        items.push(syn::parse_quote! {
            pub const SOURCE: &'static str = #src;
        });

        for (msg, spans) in self.source.errors {
            for span in spans {
                let span = span.into();
                items.push(syn::parse_quote_spanned! {span=>
                    compile_error!(#msg);
                });
            }
        }

        items
    }
}

#[proc_macro]
pub fn wgsl(shader: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let shader = process_includes(shader.into_iter());

    let mut sourcecode = Sourcecode::new();
    sourcecode.append_tokens(shader.collect());

    let mut result = sourcecode.complete();

    result.validate();

    #[cfg(feature = "minify")]
    result.minify();

    let mut tokens = proc_macro2::TokenStream::new();
    for item in result.to_items() {
        item.to_tokens(&mut tokens);
    }
    tokens.into()
}
