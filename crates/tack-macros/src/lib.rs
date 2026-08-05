use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    Attribute, Expr, ExprLit, FnArg, ImplItem, ImplItemFn, ItemImpl, Lit, LitStr, MetaNameValue,
    Pat, PatIdent, ReturnType, Token, Type, parse_macro_input, punctuated::Punctuated,
};

#[proc_macro_attribute]
pub fn toolset(args: TokenStream, input: TokenStream) -> TokenStream {
    let attrs =
        parse_macro_input!(args with Punctuated::<MetaNameValue, Token![,]>::parse_terminated);
    let mut item = parse_macro_input!(input as ItemImpl);
    match expand_toolset(&attrs, &mut item) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

fn expand_toolset(
    attrs: &Punctuated<MetaNameValue, Token![,]>,
    item: &mut ItemImpl,
) -> syn::Result<proc_macro2::TokenStream> {
    let namespace = parse_namespace(attrs)?;
    let self_ty = item.self_ty.clone();
    let mut generated = Vec::new();
    let mut tool_structs = Vec::new();

    for impl_item in &mut item.items {
        let ImplItem::Fn(method) = impl_item else {
            continue;
        };
        let Some(tool_attr_index) = method
            .attrs
            .iter()
            .position(|attr| attr.path().is_ident("tool"))
        else {
            continue;
        };
        let tool_attr = method.attrs.remove(tool_attr_index);
        let spec = parse_tool_attr(&tool_attr)?;
        let path = join_path(namespace.as_deref(), &spec.path);
        let method_ident = method.sig.ident.clone();
        let tool_ident = format_ident!("__TackTool_{}", method_ident);
        let input_ty = method_input_type(method)?;
        let output_ty = method_output_type(method)?;
        let description = spec.description;
        let name = spec.name.unwrap_or_else(|| method_ident.to_string());

        tool_structs.push(tool_ident.clone());
        generated.push(quote! {
            #[allow(non_camel_case_types)]
            struct #tool_ident {
                inner: #self_ty,
            }

            #[async_trait::async_trait]
            impl tack_core::TypedTool for #tool_ident {
                type Input = #input_ty;
                type Output = #output_ty;

                fn descriptor(&self) -> tack_core::ToolDescriptor {
                    tack_core::ToolDescriptor::new(#path, #name)
                        .with_description(#description)
                }

                async fn call_typed(
                    &self,
                    input: Self::Input,
                ) -> tack_core::ToolResult<Self::Output> {
                    self.inner.#method_ident(input).await
                }
            }
        });
    }

    let original = quote! { #item };
    Ok(quote! {
        #original

        #(#generated)*

        impl tack_core::ToolSet for #self_ty {
            fn tools(
                &self,
            ) -> Result<Vec<std::sync::Arc<dyn tack_core::Tool>>, tack_core::ToolSetError> {
                Ok(vec![
                    #(
                        std::sync::Arc::new(
                            tack_core::TypedToolAdapter::new(#tool_structs {
                                inner: self.clone(),
                            })
                        ) as std::sync::Arc<dyn tack_core::Tool>
                    ),*
                ])
            }
        }
    })
}

struct ToolAttr {
    path: String,
    description: String,
    name: Option<String>,
}

fn parse_namespace(attrs: &Punctuated<MetaNameValue, Token![,]>) -> syn::Result<Option<String>> {
    let mut namespace = None;
    for attr in attrs {
        if !attr.path.is_ident("namespace") {
            return Err(syn::Error::new_spanned(attr, "unsupported toolset option"));
        }
        let Expr::Lit(ExprLit {
            lit: Lit::Str(value),
            ..
        }) = &attr.value
        else {
            return Err(syn::Error::new_spanned(
                &attr.value,
                "toolset option must be a string literal",
            ));
        };
        namespace = Some(value.value());
    }
    Ok(namespace)
}

fn parse_tool_attr(attr: &Attribute) -> syn::Result<ToolAttr> {
    let mut path = None;
    let mut description = None;
    let mut name = None;
    attr.parse_nested_meta(|meta| {
        if meta.path.is_ident("path") {
            path = Some(meta.value()?.parse::<LitStr>()?.value());
            Ok(())
        } else if meta.path.is_ident("description") {
            description = Some(meta.value()?.parse::<LitStr>()?.value());
            Ok(())
        } else if meta.path.is_ident("name") {
            name = Some(meta.value()?.parse::<LitStr>()?.value());
            Ok(())
        } else {
            Err(meta.error("unsupported tool option"))
        }
    })?;
    Ok(ToolAttr {
        path: path.ok_or_else(|| syn::Error::new_spanned(attr, "missing `path`"))?,
        description: description
            .ok_or_else(|| syn::Error::new_spanned(attr, "missing `description`"))?,
        name,
    })
}

fn method_input_type(method: &ImplItemFn) -> syn::Result<Type> {
    let mut inputs = method.sig.inputs.iter();
    match inputs.next() {
        Some(FnArg::Receiver(_)) => {}
        _ => {
            return Err(syn::Error::new_spanned(
                &method.sig,
                "tool methods must take `&self` as the first argument",
            ));
        }
    }
    let Some(FnArg::Typed(input)) = inputs.next() else {
        return Err(syn::Error::new_spanned(
            &method.sig,
            "tool methods must take exactly one typed input argument after `&self`",
        ));
    };
    if inputs.next().is_some() {
        return Err(syn::Error::new_spanned(
            &method.sig,
            "tool methods must take exactly one typed input argument after `&self`",
        ));
    }
    match input.pat.as_ref() {
        Pat::Ident(PatIdent { .. }) => Ok((*input.ty).clone()),
        _ => Err(syn::Error::new_spanned(
            &input.pat,
            "tool input argument must be an identifier pattern",
        )),
    }
}

fn method_output_type(method: &ImplItemFn) -> syn::Result<Type> {
    let ReturnType::Type(_, ty) = &method.sig.output else {
        return Err(syn::Error::new_spanned(
            &method.sig,
            "tool methods must return `tack_core::ToolResult<Output>`",
        ));
    };
    extract_tool_result_ok_type(ty.as_ref()).ok_or_else(|| {
        syn::Error::new_spanned(
            ty,
            "tool methods must return `tack_core::ToolResult<Output>`",
        )
    })
}

fn extract_tool_result_ok_type(ty: &Type) -> Option<Type> {
    let Type::Path(path) = ty else {
        return None;
    };
    let segment = path.path.segments.last()?;
    if segment.ident != "ToolResult" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
        return None;
    };
    let Some(syn::GenericArgument::Type(output)) = args.args.first() else {
        return None;
    };
    Some(output.clone())
}

fn join_path(namespace: Option<&str>, path: &str) -> String {
    match namespace {
        Some(namespace) if !namespace.is_empty() && !path.is_empty() => {
            format!("{namespace}.{}", path.trim_start_matches('.'))
        }
        Some(namespace) if !namespace.is_empty() => namespace.to_string(),
        _ => path.to_string(),
    }
}
