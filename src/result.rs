use std::{collections::HashMap, error::Error};

use quote::ToTokens;

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

                for (loc, extra) in e.spans() {
                    self.source
                        .push_naga_error(loc.clone(), format!("{}: {}", message, extra))
                }

                None
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

    fn make_global(
        global: &naga::GlobalVariable,
        types: &naga::UniqueArena<naga::Type>,
    ) -> Vec<syn::Item> {
        let mut global_items = Vec::new();

        if let Some(binding) = &global.binding {
            let group = binding.group;
            let binding = binding.binding;
            let ty = types.get_handle(global.ty);
            global_items.push(syn::Item::Const(syn::parse_quote! {
                pub const GROUP: u32 = #group;
            }));
            global_items.push(syn::Item::Const(syn::parse_quote! {
                pub const BINDING: u32 = #binding;
            }));

            if let Some((fields, defaults, function_body)) = match (&global.space, &ty) {
                (naga::AddressSpace::Uniform, _) => Some((
                    quote::quote! {
                        visibility: wgpu::ShaderStages,
                        has_dynamic_offset: bool,
                        min_binding_size: Option<std::num::NonZeroU64>,
                    },
                    quote::quote! {
                        visibility: wgpu::ShaderStages::all(),
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    quote::quote! {
                        wgpu::BindGroupLayoutEntry {
                            binding: BINDING,
                            visibility: descriptor.visibility,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Uniform,
                                has_dynamic_offset: descriptor.has_dynamic_offset,
                                min_binding_size: descriptor.min_binding_size,
                            },
                            count: None,
                        }
                    },
                )),
                (naga::AddressSpace::Storage { access }, _) => {
                    let read_only = !access.contains(naga::StorageAccess::STORE);
                    Some((
                        quote::quote! {
                            visibility: wgpu::ShaderStages,
                            has_dynamic_offset: bool,
                            min_binding_size: Option<std::num::NonZeroU64>,
                        },
                        quote::quote! {
                            visibility: wgpu::ShaderStages::all(),
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        quote::quote! {
                            wgpu::BindGroupLayoutEntry {
                                binding: BINDING,
                                visibility: descriptor.visibility,
                                ty: wgpu::BindingType::Buffer {
                                    ty: wgpu::BufferBindingType::Storage {read_only: #read_only},
                                    has_dynamic_offset: descriptor.has_dynamic_offset,
                                    min_binding_size: descriptor.min_binding_size,
                                },
                                count: None,
                            }
                        },
                    ))
                }

                _ => None,
            } {
                global_items.push(syn::Item::Struct(syn::parse_quote!{
                    #[doc = "All the reuqired information that the shader doesn't contain when creating a bind group entry for this global."]
                    pub struct BindGroupLayoutEntryDescriptor {
                        #fields
                    }
                }));
                global_items.push(syn::Item::Impl(syn::parse_quote! {
                    impl Default for BindGroupLayoutEntryDescriptor {
                        fn default() -> Self {
                            Self {
                                #defaults
                            }
                        }
                    }
                }));
                global_items.push(syn::Item::Fn(syn::parse_quote! {
                    #[doc = "Creates a bind group layout entry, requiring the exta information not contained in the shader."]
                    pub const fn create_bind_group_layout_entry(descriptor: BindGroupLayoutEntryDescriptor) -> wgpu::BindGroupLayoutEntry {
                        #function_body
                    }
                }));
                global_items.push(syn::Item::Const(syn::parse_quote! {
                    #[doc = "A bind group entry with sensable defaults."]
                    pub const DEFAULT_BIND_GROUP_LAYOUT_ENTRY: wgpu::BindGroupLayoutEntry = create_bind_group_layout_entry(BindGroupLayoutEntryDescriptor {
                        #defaults
                    });
                }));
            }
        }

        return global_items;
    }

    fn make_globals(
        module_globals: &naga::Arena<naga::GlobalVariable>,
        types: &naga::UniqueArena<naga::Type>,
    ) -> Vec<syn::Item> {
        let mut globals = Vec::new();

        // Info about each global individually
        for (_, global) in module_globals.iter() {
            // Get name for global module
            let global_name = match &global.name {
                Some(name) => name.clone(),
                None => continue,
            };
            let global_name_ident = quote::format_ident!("{}", global_name);

            // Make items within module
            let global_items = Self::make_global(global, types);

            // Collate into an inner module
            if global_items.len() != 0 {
                let mut items_stream = proc_macro2::TokenStream::new();
                for item in global_items {
                    item.to_tokens(&mut items_stream);
                }
                let doc = format!(
                    "Information about the `{}` global variable within this shader module.",
                    global_name
                );
                globals.push(syn::parse_quote! {
                    #[doc = #doc]
                    pub mod #global_name_ident {
                        #items_stream
                    }
                })
            }
        }

        // Info about all globals together
        let mut groups = HashMap::new();
        for (_, global) in module_globals.iter() {
            if let Some(binding) = &global.binding {
                groups.entry(binding.group).or_insert(vec![]).push(global)
            }
        }
        //TODO: Create `create_bind_groups` ctr function

        return globals;
    }

    pub(crate) fn to_items(&mut self) -> Vec<syn::Item> {
        let mut items = Vec::new();

        // Globals
        let mut globals = proc_macro2::TokenStream::new();
        for global in Self::make_globals(&self.module.global_variables, &self.module.types) {
            global.to_tokens(&mut globals);
        }
        items.push(syn::parse_quote! {
            #[doc = "Information about the globals within the module, exposed as constants and functions."]
            pub mod globals {
                #globals
            }
        });

        // Errors
        for (msg, spans) in self.source.errors() {
            for span in spans {
                let span = span.clone().into();
                items.push(syn::parse_quote_spanned! {span=>
                    compile_error!(#msg);
                });
            }
            // If an error doesn't have a location, just report it everywhere
            if spans.len() == 0 {
                items.push(syn::parse_quote! {
                    compile_error!(#msg);
                });
            }
        }

        // Source string
        // This must be done last since we want to minify only after everything else has been generated about the shader.
        #[cfg(feature = "minify")]
        self.minify();
        let src = &self.source.src();
        items.push(syn::parse_quote! {
            #[doc = "The sourcecode for the shader, as a constant string."]
            pub const SOURCE: &'static str = #src;
        });

        items
    }
}
