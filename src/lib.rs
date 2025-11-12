use utoipa::openapi::{path::PathItem, Components, OpenApi, OpenApiBuilder, PathsBuilder};

/// Re-export inventory for macro use
#[doc(hidden)]
pub use inventory;

/// Re-export the route macro from macros crate
pub use food_openapi_rs_macros::route;

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

/// Build `OpenAPI` documentation from collected API entries
#[must_use]
pub fn build_openapi(title: &str, version: &str) -> OpenApi {
    build_openapi_with_components(title, version, Vec::new())
}

/// Build `OpenAPI` documentation with additional component schemas
///
/// This is useful for adding common schemas like error responses that are referenced
/// by routes but not automatically collected (e.g., via #[response] attributes).
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
///     vec![("ErrorResponse", ErrorResponse::schema())]
/// );
/// ```
#[must_use]
pub fn build_openapi_with_components<'a>(
    title: &str,
    version: &str,
    additional_schemas: impl IntoIterator<
        Item = (&'a str, utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>),
    >,
) -> OpenApi {
    let mut paths_builder = PathsBuilder::new();
    let mut components = Components::default();

    // Collect all API entries
    for entry in inventory::iter::<ApiEntry> {
        // Add path
        paths_builder = paths_builder.path(entry.path, (entry.path_item)());

        // Add schemas
        for (name, schema) in (entry.schemas)() {
            components.schemas.insert(name.to_string(), schema);
        }
    }

    // Add additional schemas
    for (name, schema) in additional_schemas {
        components.schemas.insert(name.to_string(), schema);
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
