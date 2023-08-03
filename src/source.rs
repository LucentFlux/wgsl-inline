use std::{cmp::Ordering, error::Error};

/*use naga_oil::compose::{
    ComposableModuleDescriptor, Composer, NagaModuleDescriptor, ShaderLanguage,
};
use regex::Regex;*/

use crate::result::ShaderResult;

/*fn get_shader_extension(path: &PathBuf) -> Option<ShaderLanguage> {
    match path.extension().and_then(OsStr::to_str) {
        None => None,
        Some(v) => match v {
            "wgsl" => Some(ShaderLanguage::Wgsl),
            "glsl" => Some(ShaderLanguage::Glsl),
            _ => None,
        },
    }
}

fn is_shader_extension(path: &PathBuf) -> bool {
    get_shader_extension(path).is_some()
}

fn all_child_shaders(root: PathBuf, paths: &mut Vec<PathBuf>) {
    let read = match root.read_dir() {
        Ok(fs) => fs,
        Err(e) => panic!(
            "could not read source directory {}: {:?}",
            root.display(),
            e
        ),
    };
    for file in read {
        let file = match file {
            Ok(file) => file,
            Err(e) => panic!("could not read source entry: {}", e),
        };

        let path = file.path();
        if path.is_file() && is_shader_extension(&path) {
            paths.push(file.path())
        } else if file.path().is_dir() {
            all_child_shaders(file.path(), paths);
        }
    }
}

fn all_shaders_in_project() -> Vec<(PathBuf, String)> {
    let root = std::env::var("CARGO_MANIFEST_DIR").expect("proc macros should be run using cargo");
    let src_root = std::path::Path::new(&root).join("src");

    assert!(
        src_root.exists() && src_root.is_dir(),
        "could not find source directory when composing shader"
    );

    let mut paths = Vec::new();
    all_child_shaders(src_root.clone(), &mut paths);

    paths
        .into_iter()
        .map(move |path| {
            let mut module_name = String::from("crate");

            for part in path
                .strip_prefix(&src_root)
                .expect("all child paths are children")
                .to_path_buf()
                .components()
            {
                module_name += "::";
                module_name += &part
                    .as_os_str()
                    .to_string_lossy()
                    .split(".")
                    .next()
                    .expect("all strings have at least one part")
            }

            (path, module_name)
        })
        .collect()
}*/

#[derive(Debug)]
struct SpanInString {
    start: usize,
    end: usize,
    token_span: proc_macro::Span,
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
    if last == '-' && next == '>' {
        return false; // Might be a function return like `->`
    }
    if last == next && non_identifier_char(next) {
        return false; // Might be a double operator like `++`
    }
    if last == ':' || next == ':' {
        return false; // Might be an import path like `a::b`
    }
    return true;
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
            .map(|span| span.token_span.clone())
            .collect()
    }

    pub(crate) fn push_naga_error(&mut self, loc: naga::Span, msg: String) {
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

    /// Uses naga_oil to process includes
    /*pub(crate) fn compose(&mut self) -> Option<naga::Module> {
        const INLINE_PATH: &'static str = "inline.wgsl";

        let mut composer = Composer::default();

        for (path, module) in all_shaders_in_project() {
            let language = match get_shader_extension(&path) {
                None => continue,
                Some(language) => language,
            };

            let source = match std::fs::read_to_string(&path) {
                Ok(source) => source,
                Err(_) => continue,
            };

            let res = composer.add_composable_module(ComposableModuleDescriptor {
                source: &source,
                file_path: &path.to_string_lossy(),
                language,
                as_name: Some(module.clone()),
                additional_imports: &[],
                shader_defs: HashMap::default(),
            });

            if let Err(e) = res {
                // Only report errors if the module is used here
                if let Some(pos) = self.src.find(&module) {
                    self.push_naga_error(
                        naga::Span::new(pos as u32, (pos + module.len()) as u32),
                        format! {"{}", e},
                    )
                }
            }
        }

        let res = composer.make_naga_module(NagaModuleDescriptor {
            source: &self.src,
            file_path: INLINE_PATH,
            shader_type: naga_oil::compose::ShaderType::Wgsl,
            shader_defs: HashMap::new(),
            additional_imports: &[],
        });

        match res {
            Ok(module) => Some(module),
            Err(e) => {
                let mut e_base: &dyn Error = &e;
                let mut message = format!("{}", e);
                while let Some(e) = e_base.source() {
                    message = format!("{}: {}", message, e);
                    e_base = e;
                }

                match e.source {
                    naga_oil::compose::ErrSource::Module(module_name, offset) => {
                        let module_source = &composer
                            .module_sets
                            .get(&module_name)
                            .expect("module errored so should be present in composer")
                            .substituted_source;

                        // Report error on module import
                        if let Some(pos) = self.src.find(&module_name) {
                            self.push_naga_error(
                                naga::Span::new(pos as u32, (pos + module_name.len()) as u32),
                                format!{"naga oil module error in module {} at position {}: {}", module_name, offset, message},
                            )
                        } else {
                            self.push_naga_error(naga::Span::new(0, 1),
                                format!{"naga oil module error in unknown/unfound module {} at position {}: {}", module_name, offset, message},
                            )
                        }
                    }
                    naga_oil::compose::ErrSource::Constructing {
                        path,
                        source,
                        offset,
                    } => {
                        if path == INLINE_PATH {
                            self.push_naga_error(
                                naga::Span::new(offset as u32, (offset + 1) as u32),
                                format!{"naga oil construction error in top level shader: {} offset {}", message, offset},
                            )
                        } else {
                            // Report error on module import
                            if let Some(pos) = self.src.find(&source) {
                                self.push_naga_error(
                                    naga::Span::new(pos as u32, (pos + source.len()) as u32),
                                format!{"naga oil construction error in imported module {} at position {}: {}", source, offset, message},
                                )
                            } else {
                                self.push_naga_error(naga::Span::new(0, 1),
                                format!{"naga oil construction error in unknown/unfound module {} at position {}: {}", source, offset, message},
                                )
                            }
                        }
                    }
                }

                None
            }
        }
    }*/

    pub(crate) fn complete(mut self) -> ShaderResult {
        let module = self.parse().unwrap_or(naga::Module::default());

        ShaderResult::new(self, module)
    }

    pub(crate) fn errors(&self) -> impl Iterator<Item = &(String, Vec<proc_macro::Span>)> {
        self.errors.iter()
    }

    pub(crate) fn src(&self) -> &str {
        &self.src
    }
}
