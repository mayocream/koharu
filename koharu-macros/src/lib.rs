//! Procedural macros for koharu API endpoints.

use darling::{FromMeta, ast::NestedMeta};
use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{
    FnArg, GenericArgument, Ident, ItemFn, Pat, PatIdent, PatType, PathArguments, ReturnType,
    Token, Type, parse_macro_input, punctuated::Punctuated, spanned::Spanned,
};

#[derive(Debug, FromMeta)]
struct EndpointArgs {
    path: String,
    method: String,
}

/// Marks a function as an API endpoint.
///
/// Generates the handler function with proper axum extractors and a module
/// with PATH and METHOD constants that can be used by the `routes!` macro.
///
/// # Example
/// ```ignore
/// #[endpoint(path = "/api/users", method = "get")]
/// async fn get_users(state: ApiState) -> Result<Vec<User>> {
///     // ...
/// }
///
/// // Use with routes! macro:
/// routes!(get_users, create_user, delete_user)
/// ```
#[proc_macro_attribute]
pub fn endpoint(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attr_args = match NestedMeta::parse_meta_list(attr.into()) {
        Ok(v) => v,
        Err(e) => return TokenStream::from(e.to_compile_error()),
    };

    let args = match EndpointArgs::from_list(&attr_args) {
        Ok(v) => v,
        Err(e) => return TokenStream::from(e.write_errors()),
    };

    let input = parse_macro_input!(item as ItemFn);

    match generate_endpoint(args, input) {
        Ok(tokens) => tokens.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

#[derive(Debug)]
struct ParsedParam {
    name: Ident,
    ty: Type,
    kind: ParamKind,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ParamKind {
    State,
    Payload,
}

fn extract_result_inner(ty: &Type) -> Option<&Type> {
    if let Type::Path(type_path) = ty {
        if let Some(segment) = type_path.path.segments.last() {
            if segment.ident == "Result" {
                if let PathArguments::AngleBracketed(args) = &segment.arguments {
                    if let Some(GenericArgument::Type(inner)) = args.args.first() {
                        return Some(inner);
                    }
                }
            }
        }
    }
    None
}

fn classify_param(ty: &Type) -> ParamKind {
    let type_str = quote!(#ty).to_string();
    if type_str.contains("ApiState") {
        ParamKind::State
    } else {
        ParamKind::Payload
    }
}

fn generate_endpoint(args: EndpointArgs, input: ItemFn) -> syn::Result<TokenStream2> {
    let fn_name = &input.sig.ident;
    let fn_vis = &input.vis;
    let fn_block = &input.block;
    let fn_attrs = &input.attrs;

    let return_type = match &input.sig.output {
        ReturnType::Default => {
            return Err(syn::Error::new(
                input.sig.span(),
                "endpoint must have a return type",
            ));
        }
        ReturnType::Type(_, ty) => ty.as_ref().clone(),
    };

    let inner_type = extract_result_inner(&return_type)
        .ok_or_else(|| syn::Error::new(input.sig.span(), "endpoint must return Result<T>"))?;

    let inner_str = quote!(#inner_type).to_string().replace(' ', "");
    let is_response = inner_str.contains("Response");

    let mut params = Vec::new();
    for arg in &input.sig.inputs {
        match arg {
            FnArg::Receiver(_) => return Err(syn::Error::new(arg.span(), "self not allowed")),
            FnArg::Typed(PatType { pat, ty, .. }) => {
                let name = match pat.as_ref() {
                    Pat::Ident(PatIdent { ident, .. }) => ident.clone(),
                    _ => return Err(syn::Error::new(pat.span(), "expected identifier")),
                };
                let kind = classify_param(ty);
                params.push(ParsedParam {
                    name,
                    ty: ty.as_ref().clone(),
                    kind,
                });
            }
        }
    }

    let payload_fields: Vec<_> = params
        .iter()
        .filter(|p| p.kind == ParamKind::Payload)
        .collect();
    let has_payload = !payload_fields.is_empty();

    let payload_struct_name = format_ident!("{}Payload", to_pascal_case(&fn_name.to_string()));
    let payload_struct = if has_payload {
        let fields = payload_fields.iter().map(|p| {
            let name = &p.name;
            let ty = &p.ty;
            quote! { pub #name: #ty }
        });
        quote! {
            #[derive(Debug, serde::Deserialize)]
            #[serde(rename_all = "camelCase")]
            pub struct #payload_struct_name {
                #(#fields),*
            }
        }
    } else {
        quote! {}
    };

    let mut sig_params = vec![quote! { State(state): State<ApiState> }];
    if has_payload {
        sig_params.push(quote! { Json(payload): Json<#payload_struct_name> });
    }

    let extractions: Vec<_> = payload_fields
        .iter()
        .map(|p| {
            let name = &p.name;
            quote! { let #name = payload.#name; }
        })
        .collect();

    let (final_return_type, body) = if is_response {
        // Response passthrough for file downloads
        let body = quote! {
            #(#extractions)*
            let __res: anyhow::Result<Response> = (|| async #fn_block)().await;
            __res.map_err(ApiError::from)
        };
        (quote! { ApiResult<Response> }, body)
    } else {
        // JSON response (default)
        let body = quote! {
            #(#extractions)*
            let __res: anyhow::Result<#inner_type> = (|| async #fn_block)().await;
            Ok(Json(__res.map_err(ApiError::from)?))
        };
        (quote! { ApiResult<Json<#inner_type>> }, body)
    };

    let path = &args.path;
    let method_str = &args.method;

    let output = quote! {
        #payload_struct

        #(#fn_attrs)*
        #fn_vis async fn #fn_name(#(#sig_params),*) -> #final_return_type {
            #body
        }

        #[doc(hidden)]
        #[allow(non_camel_case_types)]
        pub mod #fn_name {
            pub const PATH: &str = #path;
            pub const METHOD: &str = #method_str;
        }
    };

    Ok(output)
}

fn to_pascal_case(s: &str) -> String {
    s.split('_')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().chain(c).collect(),
            }
        })
        .collect()
}

/// Generates an axum Router from a list of endpoint functions.
///
/// Each function must be annotated with `#[endpoint]` which generates
/// a module with PATH and METHOD constants.
///
/// # Example
/// ```ignore
/// let router = routes!(
///     app_version,
///     get_documents,
///     open_documents,
/// );
/// ```
#[proc_macro]
pub fn routes(input: TokenStream) -> TokenStream {
    let names = parse_macro_input!(input with Punctuated::<Ident, Token![,]>::parse_terminated);

    let route_calls: Vec<_> = names
        .iter()
        .map(|name| {
            quote! {
                .route(#name::PATH, __build_method_router(#name::METHOD, #name))
            }
        })
        .collect();

    let output = quote! {
        {
            fn __build_method_router<H, T>(method: &str, handler: H) -> axum::routing::MethodRouter<ApiState>
            where
                H: axum::handler::Handler<T, ApiState> + Copy,
                T: 'static,
            {
                let methods: Vec<&str> = method.split(',').map(|s| s.trim()).collect();
                let mut router: Option<axum::routing::MethodRouter<ApiState>> = None;

                for m in methods {
                    router = Some(match router {
                        None => match m.to_lowercase().as_str() {
                            "get" => axum::routing::get(handler),
                            "post" => axum::routing::post(handler),
                            "put" => axum::routing::put(handler),
                            "delete" => axum::routing::delete(handler),
                            "patch" => axum::routing::patch(handler),
                            _ => axum::routing::post(handler),
                        },
                        Some(r) => match m.to_lowercase().as_str() {
                            "get" => r.get(handler),
                            "post" => r.post(handler),
                            "put" => r.put(handler),
                            "delete" => r.delete(handler),
                            "patch" => r.patch(handler),
                            _ => r.post(handler),
                        },
                    });
                }

                router.unwrap_or_else(|| axum::routing::post(handler))
            }

            Router::new()
                #(#route_calls)*
        }
    };

    output.into()
}
