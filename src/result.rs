use std::error::Error;

use crate::source::Sourcecode;

/// The output of the transformations provided by this crate.
pub(crate) struct ShaderResult {
    source: Sourcecode,
    module: naga::Module,
}

impl ShaderResult {
    pub(crate) fn new(source: Sourcecode, module: naga::Module) -> Self {
        Self { source, module }
    }

    pub(crate) fn validate(&mut self) -> Option<naga::valid::ModuleInfo> {
        let mut validator = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        );
        match validator.validate(&self.module) {
            Ok(info) => Some(info),
            Err(e) => {
                let mut e_base: &dyn Error = e.as_inner();
                let mut message = format!("{}", e);
                while let Some(e) = e_base.source() {
                    message = format!("{}: {}", message, e);
                    e_base = e;
                }

                if e.spans().len() == 0 {
                    self.source.push_naga_error(naga::Span::new(0, 1), message);
                } else {
                    for (loc, extra) in e.spans() {
                        self.source
                            .push_naga_error(*loc, format!("{}: {}", message, extra))
                    }
                }

                None
            }
        }
    }

    pub(crate) fn to_items(&self) -> Vec<syn::Item> {
        let mut items = Vec::new();

        // Errors
        for (msg, spans) in self.source.errors() {
            for span in spans {
                let span = (*span).into();
                items.push(syn::parse_quote_spanned! {span=>
                    compile_error!(#msg);
                });
            }
            // If an error doesn't have a location, just report it everywhere
            if spans.is_empty() {
                items.push(syn::parse_quote! {
                    compile_error!(#msg);
                });
            }
        }

        let mut module_items = naga_to_tokenstream::ModuleToTokens::to_items(
            &self.module,
            naga_to_tokenstream::ModuleToTokensConfig {
                structs_filter: None,
                gen_encase: cfg!(feature = "encase"),
                gen_naga: cfg!(feature = "naga"),
                gen_glam: cfg!(feature = "glam"),
            },
        );
        items.append(&mut module_items);

        items
    }
}
