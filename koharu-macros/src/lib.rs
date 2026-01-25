//! Procedural macros for koharu API endpoints.

mod endpoint;

use darling::{FromMeta, ast::NestedMeta};
use proc_macro::TokenStream;
use syn::{Ident, ItemFn, Token, parse_macro_input, punctuated::Punctuated};

/// Marks a function as an API endpoint.
///
/// Generates the handler function with proper axum extractors and a module
/// with PATH and METHOD constants that can be used by the `routes!` macro.
///
/// # Extractors
///
/// Parameters are automatically classified as extractors:
/// - `ApiState` -> State extractor (required, must be first)
/// - `Multipart` -> Multipart form data extractor
/// - Other types -> JSON payload fields
///
/// # Example
/// ```ignore
/// #[endpoint(path = "/api/users", method = "get")]
/// async fn get_users(state: ApiState) -> Result<Vec<User>> {
///     // ...
/// }
///
/// #[endpoint(path = "/api/upload", method = "post")]
/// async fn upload(state: ApiState, multipart: Multipart) -> Result<usize> {
///     // Handle multipart upload
/// }
///
/// // Use with routes! macro:
/// routes!(get_users, upload)
/// ```
#[proc_macro_attribute]
pub fn endpoint(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attr_args = match NestedMeta::parse_meta_list(attr.into()) {
        Ok(v) => v,
        Err(e) => return TokenStream::from(e.to_compile_error()),
    };

    let args = match endpoint::EndpointArgs::from_list(&attr_args) {
        Ok(v) => v,
        Err(e) => return TokenStream::from(e.write_errors()),
    };

    let input = parse_macro_input!(item as ItemFn);

    match endpoint::generate_endpoint(args, input) {
        Ok(tokens) => tokens.into(),
        Err(e) => e.to_compile_error().into(),
    }
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
    endpoint::generate_routes(names.iter()).into()
}
