use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::{parse_macro_input, FnArg, ItemFn, LitStr, PatType, ReturnType, Type};

// Constants for magic values
const CONTENT_TYPE_JSON: &str = "application/json";
const STATUS_OK: &str = "200";
const STATUS_BAD_REQUEST: &str = "400";
const STATUS_INTERNAL_ERROR: &str = "500";
const ERROR_RESPONSE_REF: &str = "ErrorResponse";

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

/// Supported HTTP methods
enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
    Patch,
}

impl HttpMethod {
    fn from_ident(ident: &syn::Ident) -> Result<Self, syn::Error> {
        let method_str = ident.to_string().to_lowercase();
        match method_str.as_str() {
            "get" => Ok(Self::Get),
            "post" => Ok(Self::Post),
            "put" => Ok(Self::Put),
            "delete" => Ok(Self::Delete),
            "patch" => Ok(Self::Patch),
            _ => Err(syn::Error::new_spanned(
                ident,
                format!(
                    "Unsupported HTTP method '{method_str}'. Supported: GET, POST, PUT, DELETE, PATCH"
                ),
            )),
        }
    }

    fn axum_routing(&self) -> TokenStream2 {
        match self {
            Self::Get => quote! { axum::routing::get },
            Self::Post => quote! { axum::routing::post },
            Self::Put => quote! { axum::routing::put },
            Self::Delete => quote! { axum::routing::delete },
            Self::Patch => quote! { axum::routing::patch },
        }
    }

    fn utoipa_method(&self) -> TokenStream2 {
        match self {
            Self::Get => quote! { utoipa::openapi::path::HttpMethod::Get },
            Self::Post => quote! { utoipa::openapi::path::HttpMethod::Post },
            Self::Put => quote! { utoipa::openapi::path::HttpMethod::Put },
            Self::Delete => quote! { utoipa::openapi::path::HttpMethod::Delete },
            Self::Patch => quote! { utoipa::openapi::path::HttpMethod::Patch },
        }
    }
}

/// Extract inner type from a generic wrapper (e.g., Json<T>, Query<T>)
fn extract_inner_type(ty: &Type, wrapper_name: &str) -> Option<Type> {
    let type_path = match ty {
        Type::Path(type_path) => type_path,
        _ => return None,
    };

    let segment = type_path.path.segments.last()?;
    if segment.ident != wrapper_name {
        return None;
    }

    let args = match &segment.arguments {
        syn::PathArguments::AngleBracketed(args) => args,
        _ => return None,
    };

    match args.args.first() {
        Some(syn::GenericArgument::Type(inner_type)) => Some(inner_type.clone()),
        _ => None,
    }
}

/// Extract type from function parameters wrapped in a specific container
fn extract_wrapper_type(fn_item: &ItemFn, wrapper_name: &str) -> Option<Type> {
    fn_item
        .sig
        .inputs
        .iter()
        .filter_map(|arg| match arg {
            FnArg::Typed(PatType { ty, .. }) => Some(&**ty),
            _ => None,
        })
        .find_map(|ty| extract_inner_type(ty, wrapper_name))
}

/// Extract request body type from function signature (Json<T>)
fn extract_request_type(fn_item: &ItemFn) -> Option<Type> {
    extract_wrapper_type(fn_item, "Json")
}

/// Extract query parameter type from function signature (Query<T>)
fn extract_query_type(fn_item: &ItemFn) -> Option<Type> {
    extract_wrapper_type(fn_item, "Query")
}

/// Check if a type is the unit type ()
fn is_unit_type(ty: &Type) -> bool {
    matches!(ty, Type::Tuple(tuple) if tuple.elems.is_empty())
}

/// Try to extract Json<T> from a type
fn try_extract_json(ty: &Type) -> Option<Type> {
    extract_inner_type(ty, "Json").filter(|t| !is_unit_type(t))
}

/// Try to extract T from Result<Json<T>, E>
fn try_extract_result_json(ty: &Type) -> Option<Type> {
    let result_inner = extract_inner_type(ty, "Result")?;
    try_extract_json(&result_inner)
}

/// Extract response body type from Result<Json<T>, E> or Json<T>
/// Returns None if the type is ()
fn extract_response_type(fn_item: &ItemFn) -> Option<Type> {
    let return_type = match &fn_item.sig.output {
        ReturnType::Type(_, ty) => &**ty,
        _ => return None,
    };

    // Try Json<T> first, then Result<Json<T>, E>
    try_extract_json(return_type).or_else(|| try_extract_result_json(return_type))
}

/// Configuration for building OpenAPI operation
struct OperationConfig<'a> {
    request_type: Option<&'a Type>,
    query_type: Option<&'a Type>,
    response_type: Option<&'a Type>,
}

impl OperationConfig<'_> {
    fn build_request_body(&self) -> Option<TokenStream2> {
        self.request_type.map(|req_type| {
            quote! {
                Some(utoipa::openapi::request_body::RequestBodyBuilder::new()
                    .content(
                        #CONTENT_TYPE_JSON,
                        utoipa::openapi::ContentBuilder::new()
                            .schema(Some(<#req_type as utoipa::PartialSchema>::schema()))
                            .build()
                    )
                    .build())
            }
        })
    }

    fn build_parameters(&self) -> Option<TokenStream2> {
        self.query_type.map(|query_type| {
            quote! {
                Some(<#query_type as utoipa::IntoParams>::into_params(|| None))
            }
        })
    }

    fn build_success_response(&self) -> TokenStream2 {
        if let Some(resp_type) = self.response_type {
            quote! {
                utoipa::openapi::ResponseBuilder::new()
                    .description("")
                    .content(
                        #CONTENT_TYPE_JSON,
                        utoipa::openapi::ContentBuilder::new()
                            .schema(Some(<#resp_type as utoipa::PartialSchema>::schema()))
                            .build()
                    )
                    .build()
            }
        } else {
            quote! {
                utoipa::openapi::ResponseBuilder::new()
                    .description("")
                    .build()
            }
        }
    }

    fn build_operation(&self) -> TokenStream2 {
        let request_body = self.build_request_body();
        let parameters = self.build_parameters();
        let success_response = self.build_success_response();

        let mut builder = quote! {
            utoipa::openapi::path::OperationBuilder::new()
        };

        if let Some(req_body) = request_body {
            builder = quote! {
                #builder.request_body(#req_body)
            };
        }

        if let Some(params) = parameters {
            builder = quote! {
                #builder.parameters(#params)
            };
        }

        quote! {
            {
                use super::*;
                #builder
                    .response(#STATUS_OK, #success_response)
                    .response(#STATUS_BAD_REQUEST, utoipa::openapi::Ref::from_response_name(#ERROR_RESPONSE_REF))
                    .response(#STATUS_INTERNAL_ERROR, utoipa::openapi::Ref::from_response_name(#ERROR_RESPONSE_REF))
                    .build()
            }
        }
    }
}

/// Attribute macro for marking route handlers
/// Usage: #[route(POST "/account/register")]
#[proc_macro_attribute]
pub fn route(args: TokenStream, input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args as RouteArgs);
    let fn_item = parse_macro_input!(input as ItemFn);

    // Parse HTTP method
    let http_method = match HttpMethod::from_ident(&args.method) {
        Ok(method) => method,
        Err(err) => return err.to_compile_error().into(),
    };

    let path = args.path.value();
    let fn_name = &fn_item.sig.ident;

    // Extract types from function signature
    let request_type = extract_request_type(&fn_item);
    let query_type = extract_query_type(&fn_item);
    let response_type = extract_response_type(&fn_item);

    // Generate routing method
    let route_method = http_method.axum_routing();
    let utoipa_method = http_method.utoipa_method();

    // Generate schema collection
    let mut schema_types = Vec::new();
    if let Some(ref req_type) = request_type {
        schema_types.push(req_type);
    }
    if let Some(ref query_type) = query_type {
        schema_types.push(query_type);
    }
    if let Some(ref resp_type) = response_type {
        schema_types.push(resp_type);
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
                            // SAFETY: We intentionally leak memory here for 'static lifetime.
                            // Schema names are typically few and small (e.g., "UserRequest", "LoginResponse"),
                            // so the memory overhead is negligible for the lifetime of the program.
                            (Box::leak(name.into_owned().into_boxed_str()) as &'static str, schema)
                        }
                    ),*
                ]
            }
        }
    };

    let meta_mod_name = format_ident!("__{}_meta", fn_name);

    // Build OpenAPI operation using the configuration
    let operation_config = OperationConfig {
        request_type: request_type.as_ref(),
        query_type: query_type.as_ref(),
        response_type: response_type.as_ref(),
    };
    let operation_builder = operation_config.build_operation();

    let path_item_builder = quote! {
        utoipa::openapi::path::PathItemBuilder::new()
            .operation(#utoipa_method, #operation_builder)
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
