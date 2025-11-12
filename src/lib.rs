use utoipa::openapi::{path::PathItem, Components, OpenApi, OpenApiBuilder, PathsBuilder};

/// Re-export inventory for macro use
#[doc(hidden)]
pub use inventory;

/// Re-export the route macro from macros crate
pub use food_openapi_rs_macros::route;

/// API entry that will be collected by inventory
pub struct ApiEntry {
    pub path: &'static str,
    pub path_item: fn() -> PathItem,
    pub schemas: fn() -> Vec<(&'static str, utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>)>,
}

inventory::collect!(ApiEntry);

/// Build OpenAPI documentation from collected API entries
pub fn build_openapi(title: &str, version: &str) -> OpenApi {
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
