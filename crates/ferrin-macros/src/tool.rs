//! Expansion of `#[ferrin::tool]`.

use proc_macro2::Span;
use proc_macro2::TokenStream;
use quote::quote;
use syn::Expr;
use syn::ExprLit;
use syn::FnArg;
use syn::GenericArgument;
use syn::ItemFn;
use syn::Lit;
use syn::PathArguments;
use syn::ReturnType;
use syn::Type;
use syn::spanned::Spanned;

const OWNED_PARAMETERS: &str = "tool parameters must be owned types (use String instead of &str)";
const PARAMETERS: &str = "tool functions take one input parameter (an owned type implementing \
                          Deserialize and JsonSchema) and an optional ToolContext parameter";

pub(crate) fn expand(attr: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    if !attr.is_empty() {
        return Err(syn::Error::new_spanned(
            attr,
            "#[ferrin::tool] takes no arguments",
        ));
    }
    let function: ItemFn = syn::parse2(item)?;
    check_signature(&function)?;
    let input_type = input_type(&function)?;
    let takes_context = function.sig.inputs.len() == 2;

    let vis = &function.vis;
    let ident = &function.sig.ident;
    let doc_attrs: Vec<_> = function
        .attrs
        .iter()
        .filter(|attr| attr.path().is_ident("doc"))
        .collect();
    let mut inner = function.clone();
    inner.attrs.retain(|attr| !attr.path().is_ident("doc"));
    inner.vis = syn::Visibility::Inherited;
    let description = doc_comment(&function).map(|text| quote! { .description(#text) });
    let context_pattern = if takes_context {
        quote! { context }
    } else {
        quote! { _context }
    };
    let call = if takes_context {
        quote! { #ident(input, context) }
    } else {
        quote! { #ident(input) }
    };
    let future = if function.sig.asyncness.is_some() {
        call
    } else {
        quote! { ::core::future::ready(#call) }
    };
    Ok(quote! {
        #(#doc_attrs)*
        #vis fn #ident() -> ::ferrin::tool::Tool {
            #inner
            ::ferrin::tool::Tool::function::<#input_type>()
                #description
                .execute(|input: #input_type, #context_pattern: ::ferrin::tool::ToolContext| #future)
                .build()
        }
    })
}

fn check_signature(function: &ItemFn) -> syn::Result<()> {
    let sig = &function.sig;
    if let Some(constness) = &sig.constness {
        return Err(syn::Error::new(
            constness.span(),
            "tool functions cannot be const",
        ));
    }
    if let syn::Safety::Unsafe(token) = &sig.safety {
        return Err(syn::Error::new(
            token.span(),
            "tool functions cannot be unsafe",
        ));
    }
    if let Some(abi) = &sig.abi {
        return Err(syn::Error::new(
            abi.span(),
            "tool functions cannot declare an ABI",
        ));
    }
    if let Some(lifetime) = sig.generics.lifetimes().next() {
        return Err(syn::Error::new(lifetime.span(), OWNED_PARAMETERS));
    }
    if !sig.generics.params.is_empty() {
        return Err(syn::Error::new(
            sig.generics.span(),
            "tool functions cannot be generic",
        ));
    }
    if let Some(variadic) = &sig.variadic {
        return Err(syn::Error::new(
            variadic.span(),
            "tool functions cannot be variadic",
        ));
    }
    if matches!(sig.output, ReturnType::Default) {
        return Err(syn::Error::new(
            sig.paren_token.span.join(),
            "tool functions must return Result<T, ToolError>",
        ));
    }
    Ok(())
}

fn input_type(function: &ItemFn) -> syn::Result<Type> {
    let inputs = &function.sig.inputs;
    if inputs.is_empty() || inputs.len() > 2 {
        return Err(syn::Error::new(
            function.sig.paren_token.span.join(),
            PARAMETERS,
        ));
    }
    let mut types = Vec::with_capacity(inputs.len());
    for input in inputs {
        match input {
            FnArg::Receiver(receiver) => {
                return Err(syn::Error::new(
                    receiver.span(),
                    "tool functions cannot take self",
                ));
            }
            FnArg::Typed(typed) => {
                if let Some(offender) = find_reference(&typed.ty) {
                    return Err(syn::Error::new(offender, OWNED_PARAMETERS));
                }
                types.push((*typed.ty).clone());
            }
        }
    }
    Ok(types.swap_remove(0))
}

/// The span of the first reference or lifetime inside `ty`, if any.
fn find_reference(ty: &Type) -> Option<Span> {
    match ty {
        Type::Reference(reference) => Some(reference.span()),
        Type::Paren(inner) => find_reference(&inner.elem),
        Type::Group(inner) => find_reference(&inner.elem),
        Type::Array(array) => find_reference(&array.elem),
        Type::Slice(slice) => find_reference(&slice.elem),
        Type::Tuple(tuple) => tuple.elems.iter().find_map(find_reference),
        Type::Path(path) => path.path.segments.iter().find_map(|segment| {
            let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
                return None;
            };
            arguments.args.iter().find_map(|argument| match argument {
                GenericArgument::Lifetime(lifetime) => Some(lifetime.span()),
                GenericArgument::Type(inner) => find_reference(inner),
                _ => None,
            })
        }),
        _ => None,
    }
}

/// The doc comment of `function`: lines joined with newlines, common leading
/// space removed, surrounding blank lines trimmed.
fn doc_comment(function: &ItemFn) -> Option<String> {
    let lines: Vec<String> = function
        .attrs
        .iter()
        .filter(|attr| attr.path().is_ident("doc"))
        .filter_map(|attr| attr.meta.require_name_value().ok())
        .filter_map(|meta| match &meta.value {
            Expr::Lit(ExprLit {
                lit: Lit::Str(text),
                ..
            }) => Some(text.value()),
            _ => None,
        })
        .map(|line| line.strip_prefix(' ').unwrap_or(&line).to_owned())
        .collect();
    let text = lines.join("\n");
    let text = text.trim_matches('\n');
    (!text.is_empty()).then(|| text.to_owned())
}
