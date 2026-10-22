//! declares a step in one place. serde tag, derives and the `StepDecl`
//! impl all come from the name string. changing the string changes the
//! serialized tag, the registry name and the journal identity together.

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::parse::Parse;
use syn::punctuated::Punctuated;
use syn::{DeriveInput, Ident, LitStr, Token};

struct StepArgs {
    name: String,
    category: String,
    touches: Vec<String>,
}

impl Parse for StepArgs {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        let mut name: Option<String> = None;
        let mut category: Option<String> = None;
        let mut touches: Vec<String> = Vec::new();

        while !input.is_empty() {
            let ident: Ident = input.parse()?;
            let _eq: Token![=] = input.parse()?;
            match ident.to_string().as_str() {
                "name" => name = Some(input.parse::<LitStr>()?.value()),
                "category" => category = Some(input.parse::<LitStr>()?.value()),
                "touches" => {
                    let content;
                    syn::bracketed!(content in input);
                    let list = Punctuated::<LitStr, Token![,]>::parse_terminated(&content)?;
                    touches.extend(list.iter().map(LitStr::value));
                }
                other => {
                    return Err(syn::Error::new(
                        ident.span(),
                        format!(
                            "unknown `step` attribute `{other}` (expected name, category, touches)"
                        ),
                    ))
                }
            }
            if input.is_empty() {
                break;
            }
            let _comma: Token![,] = input.parse()?;
        }

        let name = name.ok_or_else(|| {
            syn::Error::new(Span::call_site(), "`step` requires `name = \"...\"`")
        })?;
        Ok(StepArgs {
            name,
            category: category.unwrap_or_else(|| "uncategorized".to_string()),
            touches,
        })
    }
}

#[proc_macro_attribute]
pub fn step(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = syn::parse_macro_input!(attr as StepArgs);
    let input = syn::parse_macro_input!(item as DeriveInput);

    if !input.generics.params.is_empty() {
        return syn::Error::new_spanned(
            &input.generics,
            "the spike `#[step]` does not support generic steps",
        )
        .to_compile_error()
        .into();
    }

    let ident = &input.ident;
    let name = &args.name;
    let category = &args.category;
    let touches: Vec<LitStr> = args
        .touches
        .iter()
        .map(|t| LitStr::new(t, Span::call_site()))
        .collect();

    let expanded = quote! {
        #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
        #[serde(tag = "step", rename = #name)]
        #input

        impl spike_kernel::StepDecl for #ident {
            const META: spike_kernel::StepMeta = spike_kernel::StepMeta {
                name: #name,
                category: #category,
                touches: &[#(#touches),*],
            };
        }
    };
    expanded.into()
}
