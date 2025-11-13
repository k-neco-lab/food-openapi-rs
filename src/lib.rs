use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use utoipa::openapi::{
    path::PathItem, Components, OpenApi, OpenApiBuilder, PathsBuilder, RefOr, Response,
};

/// Re-export inventory for macro use
#[doc(hidden)]
pub use inventory;

/// Re-export the route macro from macros crate
pub use food_openapi_rs_macros::route;

/// Standard error response structure
///
/// This is a common error response format that can be used across different APIs.
#[derive(serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct ErrorResponse {
    pub error: String,
}

/// Type alias for schema entry: (name, schema)
pub type SchemaEntry = (
    &'static str,
    utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>,
);

/// API entry that will be collected by inventory
pub struct ApiEntry {
    pub path: &'static str,
    pub path_item: fn() -> PathItem,
    pub schemas: fn() -> Vec<SchemaEntry>,
}

inventory::collect!(ApiEntry);

/// Schema provider that generates schemas at runtime
pub struct SchemaProvider {
    pub name: &'static str,
    pub provider: fn() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>,
}

inventory::collect!(SchemaProvider);

/// Helper function to extract all $ref schema names from a schema
fn extract_schema_refs(schema: &RefOr<utoipa::openapi::schema::Schema>) -> HashSet<String> {
    use utoipa::openapi::schema::Schema;

    let mut refs = HashSet::new();
    let mut queue = VecDeque::new();
    queue.push_back(schema);

    while let Some(current) = queue.pop_front() {
        match current {
            RefOr::Ref(r) => {
                if let Some(schema_name) = r.ref_location.strip_prefix("#/components/schemas/") {
                    refs.insert(schema_name.to_string());
                }
            }
            RefOr::T(Schema::Object(obj)) => {
                for prop in obj.properties.values() {
                    queue.push_back(prop);
                }
                if let Some(additional_props) = &obj.additional_properties {
                    use utoipa::openapi::schema::AdditionalProperties;
                    if let AdditionalProperties::RefOr(additional) = additional_props.as_ref() {
                        queue.push_back(additional);
                    }
                }
            }
            RefOr::T(Schema::Array(_arr)) => {
                // Array items are typically inline schemas or simple types
                // Nested refs will be collected from the parent schema
            }
            RefOr::T(Schema::OneOf(one_of)) => {
                for item in &one_of.items {
                    queue.push_back(item);
                }
            }
            RefOr::T(Schema::AnyOf(any_of)) => {
                for item in &any_of.items {
                    queue.push_back(item);
                }
            }
            RefOr::T(Schema::AllOf(all_of)) => {
                for item in &all_of.items {
                    queue.push_back(item);
                }
            }
            RefOr::T(_) => {}
        }
    }

    refs
}

/// Build an error response for use in `OpenAPI` components.responses
///
/// This is a helper function to create a standard error response that can be
/// referenced by all error status codes (400, 500, etc.).
///
/// # Example
/// ```ignore
/// use utoipa::{ToSchema, PartialSchema};
///
/// #[derive(ToSchema)]
/// struct ErrorResponse {
///     error: String,
/// }
///
/// let openapi = build_openapi_with_components(
///     "my-api",
///     "1.0.0",
///     Vec::new(),
///     Some(("ErrorResponse", build_error_response_from_schema(ErrorResponse::schema())))
/// );
/// ```
#[must_use]
pub fn build_error_response_from_schema(
    schema: utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>,
) -> Response {
    use utoipa::openapi::content::ContentBuilder;
    use utoipa::openapi::response::ResponseBuilder;

    ResponseBuilder::new()
        .description("Error response")
        .content(
            "application/json",
            ContentBuilder::new().schema(Some(schema)).build(),
        )
        .build()
}

/// Build `OpenAPI` documentation from collected API entries
#[must_use]
pub fn build_openapi(title: &str, version: &str) -> OpenApi {
    build_openapi_with_components(title, version, Vec::new(), None)
}

/// Build `OpenAPI` documentation with standard error response
///
/// This is the recommended way to build `OpenAPI` documentation.
/// It automatically includes a standard `ErrorResponse` in `components.responses`
/// and resolves all nested schema references.
///
/// # Example
/// ```ignore
/// let openapi = build_openapi_with_error_response("my-api", "1.0.0");
/// ```
#[must_use]
pub fn build_openapi_with_error_response(title: &str, version: &str) -> OpenApi {
    use utoipa::PartialSchema;

    build_openapi_with_components(
        title,
        version,
        Vec::new(),
        Some((
            "ErrorResponse",
            build_error_response_from_schema(ErrorResponse::schema()),
        )),
    )
}

/// Build `OpenAPI` documentation with additional components
///
/// This function automatically resolves nested schema references using registered
/// schema providers. Error responses should be provided separately via the
/// `error_response` parameter to be placed in `components.responses`.
///
/// # Example
/// ```ignore
/// use utoipa::{ToSchema, PartialSchema};
///
/// #[derive(ToSchema)]
/// struct ErrorResponse {
///     error: String,
/// }
///
/// let openapi = build_openapi_with_components(
///     "my-api",
///     "1.0.0",
///     Vec::new(), // additional schemas (deprecated, use schema providers instead)
///     Some(("ErrorResponse", ResponseBuilder::new()
///         .description("Error response")
///         .content("application/json", ContentBuilder::new()
///             .schema(Some(ErrorResponse::schema()))
///             .build())
///         .build()))
/// );
/// ```
#[must_use]
pub fn build_openapi_with_components<'a>(
    title: &str,
    version: &str,
    additional_schemas: impl IntoIterator<
        Item = (
            &'a str,
            utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>,
        ),
    >,
    error_response: Option<(&'a str, Response)>,
) -> OpenApi {
    let mut paths_builder = PathsBuilder::new();
    let mut components = Components::default();
    let mut schemas: BTreeMap<String, RefOr<utoipa::openapi::schema::Schema>> = BTreeMap::new();

    // Build schema provider map
    let mut schema_providers: HashMap<String, fn() -> RefOr<utoipa::openapi::schema::Schema>> =
        HashMap::new();
    for provider in inventory::iter::<SchemaProvider> {
        schema_providers.insert(provider.name.to_string(), provider.provider);
    }

    // Collect all API entries
    for entry in inventory::iter::<ApiEntry> {
        // Add path
        paths_builder = paths_builder.path(entry.path, (entry.path_item)());

        // Add schemas from entry
        for (name, schema) in (entry.schemas)() {
            schemas.insert(name.to_string(), schema);
        }
    }

    // Add additional schemas
    for (name, schema) in additional_schemas {
        schemas.insert(name.to_string(), schema);
    }

    // Recursively resolve all schema references
    let mut resolved_schemas: HashSet<String> = schemas.keys().cloned().collect();
    let mut to_process: VecDeque<String> = resolved_schemas.iter().cloned().collect();

    while let Some(schema_name) = to_process.pop_front() {
        if let Some(schema) = schemas.get(&schema_name) {
            // Extract all refs from this schema
            let refs = extract_schema_refs(schema);

            for ref_name in refs {
                if !resolved_schemas.contains(&ref_name) {
                    // Try to resolve from schema providers
                    if let Some(provider) = schema_providers.get(&ref_name) {
                        schemas.insert(ref_name.clone(), provider());
                        resolved_schemas.insert(ref_name.clone());
                        to_process.push_back(ref_name);
                    }
                }
            }
        }
    }

    components.schemas = schemas;

    // Add error response if provided
    if let Some((name, response)) = error_response {
        components
            .responses
            .insert(name.to_string(), RefOr::T(response));
    }

    OpenApiBuilder::new()
        .info(
            utoipa::openapi::InfoBuilder::new()
                .title(title)
                .version(version)
                .build(),
        )
        .paths(paths_builder.build())
        .components(Some(components))
        .build()
}
