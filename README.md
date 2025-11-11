# food-openapi-rs

A Rust procedural macro library for generating OpenAPI documentation from route handler attributes.

## Features

- Automatic OpenAPI documentation generation for Axum routes
- Support for request body, query parameters, and response types
- Built on top of `utoipa`

## Usage

Add this to your `Cargo.toml`:

```toml
[dependencies]
food-openapi-rs = "0.1.0"
```

Then use the `#[route]` attribute on your handler functions:

```rust
use food_openapi_rs::route;
use axum::Json;

#[route(POST "/account/register")]
async fn register(Json(req): Json<RegisterRequest>) -> Result<Json<RegisterResponse>, Error> {
    // Your handler implementation
}
```

## License

MIT