use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::{parse_macro_input, FnArg, ItemFn, LitStr, PatType, ReturnType, Type};

/// Parse route attribute arguments
/// Example: POST "/account/register"
struct RouteArgs {
    method: syn::Ident,
    path: LitStr,
}

impl Parse for RouteArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let method: syn::Ident = input.parse()?;
        let path: LitStr = input.parse()?;
        Ok(Self { method, path })
    }
}

/// Extract request body type from function signature
fn extract_request_type(fn_item: &ItemFn) -> Option<Type> {
    for arg in &fn_item.sig.inputs {
        if let FnArg::Typed(PatType { ty, .. }) = arg {
            if let Type::Path(type_path) = &**ty {
                let segment = type_path.path.segments.last()?;
                if segment.ident == "Json" {
                    if let syn::PathArguments::AngleBracketed(args) = &segment.arguments {
                        if let Some(syn::GenericArgument::Type(inner_type)) = args.args.first() {
                            return Some(inner_type.clone());
                        }
                    }
                }
            }
        }
    }
    None
}

/// Extract query parameter type from function signature
fn extract_query_type(fn_item: &ItemFn) -> Option<Type> {
    for arg in &fn_item.sig.inputs {
        if let FnArg::Typed(PatType { ty, .. }) = arg {
            if let Type::Path(type_path) = &**ty {
                let segment = type_path.path.segments.last()?;
                if segment.ident == "Query" {
                    if let syn::PathArguments::AngleBracketed(args) = &segment.arguments {
                        if let Some(syn::GenericArgument::Type(inner_type)) = args.args.first() {
                            return Some(inner_type.clone());
                        }
                    }
                }
            }
        }
    }
    None
}

/// Extract response body type from Result<Json<T>, E>
fn extract_response_type(fn_item: &ItemFn) -> Option<Type> {
    if let ReturnType::Type(_, ty) = &fn_item.sig.output {
        if let Type::Path(type_path) = &**ty {
            let segment = type_path.path.segments.last()?;
            if segment.ident == "Result" {
                if let syn::PathArguments::AngleBracketed(args) = &segment.arguments {
                    if let Some(syn::GenericArgument::Type(Type::Path(ok_type))) = args.args.first()
                    {
                        let json_segment = ok_type.path.segments.last()?;
                        if json_segment.ident == "Json" {
                            if let syn::PathArguments::AngleBracketed(json_args) =
                                &json_segment.arguments
                            {
                                if let Some(syn::GenericArgument::Type(inner_type)) =
                                    json_args.args.first()
                                {
                                    return Some(inner_type.clone());
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

/// Attribute macro for marking route handlers
/// Usage: #[route(POST "/account/register")]
///
/// # Panics
/// Panics if an unsupported HTTP method is provided
#[proc_macro_attribute]
#[allow(clippy::too_many_lines, clippy::option_if_let_else)]
pub fn route(args: TokenStream, input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args as RouteArgs);
    let fn_item = parse_macro_input!(input as ItemFn);

    let method = args.method.to_string().to_lowercase();
    let path = args.path.value();
    let fn_name = &fn_item.sig.ident;

    // Extract types from function signature
    let request_type = extract_request_type(&fn_item);
    let query_type = extract_query_type(&fn_item);
    let response_type = extract_response_type(&fn_item);

    // Build utoipa::path attributes
    let method_ident = format_ident!("{}", method);

    let utoipa_attrs = if let Some(req_type) = &request_type {
        if let Some(resp_type) = &response_type {
            quote! {
                #[utoipa::path(
                    #method_ident,
                    path = #path,
                    request_body = #req_type,
                    responses(
                        (status = 200, description = "", body = #resp_type),
                        (status = 400, response = crate::error::ErrorResponse),
                        (status = 500, response = crate::error::ErrorResponse)
                    )
                )]
            }
        } else {
            quote! {
                #[utoipa::path(
                    #method_ident,
                    path = #path,
                    request_body = #req_type,
                    responses(
                        (status = 200, description = ""),
                        (status = 400, response = crate::error::ErrorResponse),
                        (status = 500, response = crate::error::ErrorResponse)
                    )
                )]
            }
        }
    } else if let Some(query_type) = &query_type {
        if let Some(resp_type) = &response_type {
            quote! {
                #[utoipa::path(
                    #method_ident,
                    path = #path,
                    params(#query_type),
                    responses(
                        (status = 200, description = "", body = #resp_type),
                        (status = 400, response = crate::error::ErrorResponse),
                        (status = 500, response = crate::error::ErrorResponse)
                    )
                )]
            }
        } else {
            quote! {
                #[utoipa::path(
                    #method_ident,
                    path = #path,
                    params(#query_type),
                    responses(
                        (status = 200, description = ""),
                        (status = 400, response = crate::error::ErrorResponse),
                        (status = 500, response = crate::error::ErrorResponse)
                    )
                )]
            }
        }
    } else if let Some(resp_type) = &response_type {
        quote! {
            #[utoipa::path(
                #method_ident,
                path = #path,
                responses(
                    (status = 200, description = "", body = #resp_type),
                    (status = 400, response = crate::error::ErrorResponse),
                    (status = 500, response = crate::error::ErrorResponse)
                )
            )]
        }
    } else {
        quote! {
            #[utoipa::path(
                #method_ident,
                path = #path,
                responses(
                    (status = 200, description = ""),
                    (status = 400, response = crate::error::ErrorResponse),
                    (status = 500, response = crate::error::ErrorResponse)
                )
            )]
        }
    };

    // Generate routing method
    let route_method = match method.as_str() {
        "get" => quote! { axum::routing::get },
        "post" => quote! { axum::routing::post },
        "put" => quote! { axum::routing::put },
        "delete" => quote! { axum::routing::delete },
        "patch" => quote! { axum::routing::patch },
        _ => panic!("Unsupported HTTP method: {method}"),
    };

    let expanded = quote! {
        #utoipa_attrs
        #fn_item

        pub mod #fn_name {
            pub const ROUTE: fn(axum::Router<crate::AppState>) -> axum::Router<crate::AppState> = |router| {
                router.route(#path, #route_method(super::#fn_name))
            };
        }
    };

    TokenStream::from(expanded)
}
