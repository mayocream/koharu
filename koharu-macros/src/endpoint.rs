//! Endpoint macro implementation.

use darling::FromMeta;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{
    FnArg, GenericArgument, Ident, ItemFn, Pat, PatIdent, PatType, PathArguments, ReturnType, Type,
    spanned::Spanned,
};

#[derive(Debug, FromMeta)]
pub struct EndpointArgs {
    pub path: String,
    pub method: String,
}

// ============================================================================
// Parameter Classification
// ============================================================================

/// Known axum extractor types that receive special handling.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Extractor {
    /// State<ApiState> - application state
    State,
    /// Multipart - form data with file uploads
    Multipart,
}

impl Extractor {
    /// Try to match a type name to a known extractor.
    fn from_type_name(name: &str) -> Option<Self> {
        match name {
            "ApiState" => Some(Self::State),
            "Multipart" => Some(Self::Multipart),
            _ => None,
        }
    }

    /// Whether this extractor requires special async handling (can't be in a closure).
    fn requires_direct_async(&self) -> bool {
        matches!(self, Self::Multipart)
    }
}

/// A parsed function parameter with its classification.
#[derive(Debug)]
struct Param {
    name: Ident,
    ty: Type,
    extractor: Option<Extractor>,
}

impl Param {
    fn is_payload(&self) -> bool {
        self.extractor.is_none()
    }
}

/// Extract the inner type from Result<T>.
fn unwrap_result_type(ty: &Type) -> Option<&Type> {
    let Type::Path(type_path) = ty else {
        return None;
    };
    let segment = type_path.path.segments.last()?;
    if segment.ident != "Result" {
        return None;
    }
    let PathArguments::AngleBracketed(args) = &segment.arguments else {
        return None;
    };
    match args.args.first()? {
        GenericArgument::Type(inner) => Some(inner),
        _ => None,
    }
}

/// Get the last segment name of a type path (e.g., "Vec" from "std::vec::Vec").
fn type_name(ty: &Type) -> Option<String> {
    let Type::Path(type_path) = ty else {
        return None;
    };
    Some(type_path.path.segments.last()?.ident.to_string())
}

/// Classify a parameter type.
fn classify_param(name: Ident, ty: &Type) -> Param {
    let extractor = type_name(ty).and_then(|n| Extractor::from_type_name(&n));
    Param {
        name,
        ty: ty.clone(),
        extractor,
    }
}

// ============================================================================
// Code Generation
// ============================================================================

/// Configuration for generating the handler body.
struct BodyConfig<'a> {
    inner_type: &'a Type,
    is_response: bool,
    use_closure: bool,
    extractions: Vec<TokenStream>,
    fn_block: &'a syn::Block,
}

impl<'a> BodyConfig<'a> {
    fn generate(&self) -> (TokenStream, TokenStream) {
        let Self {
            inner_type,
            is_response,
            use_closure,
            extractions,
            fn_block,
        } = self;

        let async_expr = if *use_closure {
            quote! { (|| async #fn_block)().await }
        } else {
            quote! { async #fn_block.await }
        };

        if *is_response {
            let body = quote! {
                #(#extractions)*
                let __res: anyhow::Result<Response> = #async_expr;
                __res.map_err(ApiError::from)
            };
            (quote! { ApiResult<Response> }, body)
        } else {
            let body = quote! {
                #(#extractions)*
                let __res: anyhow::Result<#inner_type> = #async_expr;
                Ok(Json(__res.map_err(ApiError::from)?))
            };
            (quote! { ApiResult<Json<#inner_type>> }, body)
        }
    }
}

pub fn generate_endpoint(args: EndpointArgs, input: ItemFn) -> syn::Result<TokenStream> {
    let fn_name = &input.sig.ident;
    let fn_vis = &input.vis;
    let fn_block = &input.block;
    let fn_attrs = &input.attrs;

    // Parse return type
    let return_type = match &input.sig.output {
        ReturnType::Default => {
            return Err(syn::Error::new(
                input.sig.span(),
                "endpoint must have a return type",
            ));
        }
        ReturnType::Type(_, ty) => ty.as_ref(),
    };

    let inner_type = unwrap_result_type(return_type)
        .ok_or_else(|| syn::Error::new(input.sig.span(), "endpoint must return Result<T>"))?;

    let is_response = type_name(inner_type).is_some_and(|n| n == "Response");

    // Parse and classify parameters
    let params: Vec<Param> = input
        .sig
        .inputs
        .iter()
        .map(|arg| match arg {
            FnArg::Receiver(_) => Err(syn::Error::new(arg.span(), "self not allowed")),
            FnArg::Typed(PatType { pat, ty, .. }) => {
                let Pat::Ident(PatIdent { ident, .. }) = pat.as_ref() else {
                    return Err(syn::Error::new(pat.span(), "expected identifier"));
                };
                Ok(classify_param(ident.clone(), ty))
            }
        })
        .collect::<syn::Result<_>>()?;

    // Separate extractors from payload fields
    let multipart = params
        .iter()
        .find(|p| p.extractor == Some(Extractor::Multipart));
    let payload_fields: Vec<_> = params.iter().filter(|p| p.is_payload()).collect();

    // Validate: multipart and JSON payload are mutually exclusive
    if multipart.is_some() && !payload_fields.is_empty() {
        return Err(syn::Error::new(
            input.sig.span(),
            "cannot mix Multipart with JSON payload parameters",
        ));
    }

    // Generate payload struct if needed
    let payload_struct_name = format_ident!("{}Payload", to_pascal_case(&fn_name.to_string()));
    let payload_struct = if !payload_fields.is_empty() {
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

    // Generate handler signature parameters
    let mut sig_params = vec![quote! { State(state): State<ApiState> }];

    if let Some(mp) = multipart {
        let mp_name = &mp.name;
        sig_params.push(quote! { mut #mp_name: axum::extract::Multipart });
    } else if !payload_fields.is_empty() {
        sig_params.push(quote! { Json(payload): Json<#payload_struct_name> });
    }

    // Generate payload field extractions
    let extractions: Vec<_> = payload_fields
        .iter()
        .map(|p| {
            let name = &p.name;
            quote! { let #name = payload.#name; }
        })
        .collect();

    // Determine if we can use closure pattern
    // (Multipart captures body stream and can't escape FnMut closure)
    let use_closure = !params
        .iter()
        .filter_map(|p| p.extractor)
        .any(|e| e.requires_direct_async());

    // Generate handler body
    let (final_return_type, body) = BodyConfig {
        inner_type,
        is_response,
        use_closure,
        extractions,
        fn_block,
    }
    .generate();

    // Generate output
    let path = &args.path;
    let method_str = &args.method;

    Ok(quote! {
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
    })
}

fn to_pascal_case(s: &str) -> String {
    s.split('_')
        .filter_map(|w| {
            let mut chars = w.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().chain(chars).collect::<String>())
        })
        .collect()
}

// ============================================================================
// Routes Generation
// ============================================================================

pub fn generate_routes<'a>(names: impl Iterator<Item = &'a Ident>) -> TokenStream {
    let route_calls = names.map(|name| {
        quote! {
            .route(#name::PATH, __build_method_router(#name::METHOD, #name))
        }
    });

    quote! {
        {
            fn __build_method_router<H, T>(
                method: &str,
                handler: H,
            ) -> axum::routing::MethodRouter<ApiState>
            where
                H: axum::handler::Handler<T, ApiState> + Copy,
                T: 'static,
            {
                let mut router: Option<axum::routing::MethodRouter<ApiState>> = None;
                for m in method.split(',').map(str::trim) {
                    router = Some(match (router, m.to_lowercase().as_str()) {
                        (None, "get") => axum::routing::get(handler),
                        (None, "post") => axum::routing::post(handler),
                        (None, "put") => axum::routing::put(handler),
                        (None, "delete") => axum::routing::delete(handler),
                        (None, "patch") => axum::routing::patch(handler),
                        (None, _) => axum::routing::post(handler),
                        (Some(r), "get") => r.get(handler),
                        (Some(r), "post") => r.post(handler),
                        (Some(r), "put") => r.put(handler),
                        (Some(r), "delete") => r.delete(handler),
                        (Some(r), "patch") => r.patch(handler),
                        (Some(r), _) => r.post(handler),
                    });
                }
                router.unwrap_or_else(|| axum::routing::post(handler))
            }

            Router::new()
                #(#route_calls)*
        }
    }
}
