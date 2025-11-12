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

/// Check if a type is the unit type ()
fn is_unit_type(ty: &Type) -> bool {
    matches!(ty, Type::Tuple(tuple) if tuple.elems.is_empty())
}

/// Extract response body type from Result<Json<T>, E> or Json<T>
/// Returns None if the type is ()
fn extract_response_type(fn_item: &ItemFn) -> Option<Type> {
    if let ReturnType::Type(_, ty) = &fn_item.sig.output {
        if let Type::Path(type_path) = &**ty {
            let segment = type_path.path.segments.last()?;

            // Try Json<T> first
            if segment.ident == "Json" {
                if let syn::PathArguments::AngleBracketed(args) = &segment.arguments {
                    if let Some(syn::GenericArgument::Type(inner_type)) = args.args.first() {
                        // Skip unit type
                        if is_unit_type(inner_type) {
                            return None;
                        }
                        return Some(inner_type.clone());
                    }
                }
            }

            // Then try Result<Json<T>, E>
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
                                    // Skip unit type
                                    if is_unit_type(inner_type) {
                                        return None;
                                    }
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

    // Generate routing method
    let route_method = match method.as_str() {
        "get" => quote! { axum::routing::get },
        "post" => quote! { axum::routing::post },
        "put" => quote! { axum::routing::put },
        "delete" => quote! { axum::routing::delete },
        "patch" => quote! { axum::routing::patch },
        _ => panic!("Unsupported HTTP method: {method}"),
    };

    // Generate schema collection
    let mut schema_types = Vec::new();
    if let Some(req_type) = &request_type {
        schema_types.push(req_type.clone());
    }
    if let Some(query_type) = &query_type {
        schema_types.push(query_type.clone());
    }
    if let Some(resp_type) = &response_type {
        schema_types.push(resp_type.clone());
    }

    let schemas_fn = if schema_types.is_empty() {
        quote! {
            fn __schemas() -> Vec<(&'static str, utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>)> {
                Vec::new()
            }
        }
    } else {
        quote! {
            fn __schemas() -> Vec<(&'static str, utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>)> {
                use super::*;
                use std::borrow::Cow;
                vec![
                    #(
                        {
                            let name: Cow<'static, str> = <#schema_types as utoipa::ToSchema>::name();
                            let schema = <#schema_types as utoipa::PartialSchema>::schema();
                            (Box::leak(name.into_owned().into_boxed_str()) as &'static str, schema)
                        }
                    ),*
                ]
            }
        }
    };

    let meta_mod_name = format_ident!("__{}_meta", fn_name);

    // Build OpenAPI operation directly
    let operation_builder = if let Some(req_type) = &request_type {
        if let Some(resp_type) = &response_type {
            quote! {
                {
                    use super::*;
                    utoipa::openapi::path::OperationBuilder::new()
                        .request_body(Some(utoipa::openapi::request_body::RequestBodyBuilder::new()
                            .content(
                                "application/json",
                                utoipa::openapi::ContentBuilder::new()
                                    .schema(Some(<#req_type as utoipa::PartialSchema>::schema()))
                                    .build()
                            )
                            .build()))
                        .response(
                            "200",
                            utoipa::openapi::ResponseBuilder::new()
                                .description("")
                                .content(
                                    "application/json",
                                    utoipa::openapi::ContentBuilder::new()
                                        .schema(Some(<#resp_type as utoipa::PartialSchema>::schema()))
                                        .build()
                                )
                                .build()
                        )
                        .response("400", utoipa::openapi::Ref::from_response_name("ErrorResponse"))
                        .response("500", utoipa::openapi::Ref::from_response_name("ErrorResponse"))
                        .build()
                }
            }
        } else {
            quote! {
                {
                    use super::*;
                    utoipa::openapi::path::OperationBuilder::new()
                        .request_body(Some(utoipa::openapi::request_body::RequestBodyBuilder::new()
                            .content(
                                "application/json",
                                utoipa::openapi::ContentBuilder::new()
                                    .schema(Some(<#req_type as utoipa::PartialSchema>::schema()))
                                    .build()
                            )
                            .build()))
                        .response("200", utoipa::openapi::ResponseBuilder::new().description("").build())
                        .response("400", utoipa::openapi::Ref::from_response_name("ErrorResponse"))
                        .response("500", utoipa::openapi::Ref::from_response_name("ErrorResponse"))
                        .build()
                }
            }
        }
    } else if let Some(query_type) = &query_type {
        if let Some(resp_type) = &response_type {
            quote! {
                {
                    use super::*;
                    utoipa::openapi::path::OperationBuilder::new()
                        .parameters(Some(<#query_type as utoipa::IntoParams>::into_params(|| None)))
                        .response(
                            "200",
                            utoipa::openapi::ResponseBuilder::new()
                                .description("")
                                .content(
                                    "application/json",
                                    utoipa::openapi::ContentBuilder::new()
                                        .schema(Some(<#resp_type as utoipa::PartialSchema>::schema()))
                                        .build()
                                )
                                .build()
                        )
                        .response("400", utoipa::openapi::Ref::from_response_name("ErrorResponse"))
                        .response("500", utoipa::openapi::Ref::from_response_name("ErrorResponse"))
                        .build()
                }
            }
        } else {
            quote! {
                {
                    use super::*;
                    utoipa::openapi::path::OperationBuilder::new()
                        .parameters(Some(<#query_type as utoipa::IntoParams>::into_params(|| None)))
                        .response("200", utoipa::openapi::ResponseBuilder::new().description("").build())
                        .response("400", utoipa::openapi::Ref::from_response_name("ErrorResponse"))
                        .response("500", utoipa::openapi::Ref::from_response_name("ErrorResponse"))
                        .build()
                }
            }
        }
    } else if let Some(resp_type) = &response_type {
        quote! {
            {
                use super::*;
                utoipa::openapi::path::OperationBuilder::new()
                    .response(
                        "200",
                        utoipa::openapi::ResponseBuilder::new()
                            .description("")
                            .content(
                                "application/json",
                                utoipa::openapi::ContentBuilder::new()
                                    .schema(Some(<#resp_type as utoipa::PartialSchema>::schema()))
                                    .build()
                            )
                            .build()
                    )
                    .response("400", utoipa::openapi::Ref::from_response_name("ErrorResponse"))
                    .response("500", utoipa::openapi::Ref::from_response_name("ErrorResponse"))
                    .build()
            }
        }
    } else {
        quote! {
            utoipa::openapi::path::OperationBuilder::new()
                .response("200", utoipa::openapi::ResponseBuilder::new().description("").build())
                .response("400", utoipa::openapi::Ref::from_response_name("ErrorResponse"))
                .response("500", utoipa::openapi::Ref::from_response_name("ErrorResponse"))
                .build()
        }
    };

    let http_method = match method.as_str() {
        "get" => quote! { utoipa::openapi::path::HttpMethod::Get },
        "post" => quote! { utoipa::openapi::path::HttpMethod::Post },
        "put" => quote! { utoipa::openapi::path::HttpMethod::Put },
        "delete" => quote! { utoipa::openapi::path::HttpMethod::Delete },
        "patch" => quote! { utoipa::openapi::path::HttpMethod::Patch },
        _ => panic!("Unsupported HTTP method: {method}"),
    };

    let path_item_builder = quote! {
        utoipa::openapi::path::PathItemBuilder::new()
            .operation(#http_method, #operation_builder)
            .build()
    };

    let expanded = quote! {
        #fn_item

        pub mod #fn_name {
            pub const ROUTE: fn(axum::Router<crate::AppState>) -> axum::Router<crate::AppState> = |router| {
                router.route(#path, #route_method(super::#fn_name))
            };
        }

        mod #meta_mod_name {
            #schemas_fn

            fn __path_item() -> utoipa::openapi::path::PathItem {
                #path_item_builder
            }

            food_openapi_rs::inventory::submit! {
                food_openapi_rs::ApiEntry {
                    path: #path,
                    path_item: __path_item,
                    schemas: __schemas,
                }
            }
        }
    };

    TokenStream::from(expanded)
}
