//! Node-API bindings of `federation_composition`, built with napi-rs.
//!
//! Unlike the WebAssembly package, which takes graphql-js `DocumentNode`s, this addon takes
//! each subgraph's SDL as a string and parses it in Rust, so nothing but strings crosses the
//! boundary:
//!
//! ```js
//! const { composeServices } = require("./federation-composition.node");
//! const { supergraphSdl, errors } = composeServices([{ name: "users", sdl: "type Query { me: String }" }]);
//! ```
//!
//! `node crates/federation-composition-napi/build.mjs` builds the addon.
#![cfg(not(target_family = "wasm"))]

use federation_composition::__internal::subgraph::validation::validate_subgraph::InternalOptions as CoreInternalOptions;
use federation_composition::{CompositionResult as CoreResult, ServiceDefinition};
use napi_derive::napi;

/// A subgraph: its name, optional URL and SDL.
#[napi(object)]
pub struct Service {
    pub name: String,
    pub url: Option<String>,
    pub sdl: String,
}

/// Upstream's `__internal` argument of `composeServices`.
#[napi(object)]
pub struct InternalOptions {
    pub disable_validation_rules: Option<Vec<String>>,
}

/// A composition error: the message and `extensions.code`.
#[napi(object)]
pub struct CompositionError {
    pub message: String,
    pub code: Option<String>,
}

/// `supergraphSdl` on success, `errors` on failure.
#[napi(object)]
pub struct CompositionResult {
    pub supergraph_sdl: Option<String>,
    pub errors: Option<Vec<CompositionError>>,
}

/// Parses each subgraph's SDL and composes them: upstream's `composeServices(services, __internal?)`.
///
/// Throws where upstream throws, and for SDL that does not parse.
#[napi(catch_unwind)]
pub fn compose_services(services: Vec<Service>, internal: Option<InternalOptions>) -> napi::Result<CompositionResult> {
    let services = services
        .into_iter()
        .map(|s| {
            let type_defs = graphql_core::parse(&s.sdl).map_err(|e| napi::Error::from_reason(e.message))?;
            Ok(ServiceDefinition {
                name: s.name,
                url: s.url,
                type_defs,
            })
        })
        .collect::<napi::Result<Vec<_>>>()?;

    let result = match internal {
        // The public entry point also enforces the input limits (nesting depth).
        None => {
            federation_composition::compose_services(&services).map_err(|e| napi::Error::from_reason(e.to_string()))
        }
        // Only upstream's test suite passes `__internal`.
        Some(i) => federation_composition::__internal::compose::compose_services(
            &services,
            Some(&CoreInternalOptions {
                disable_validation_rules: i.disable_validation_rules,
            }),
        )
        .map_err(|e| napi::Error::from_reason(e.to_string())),
    }?;

    Ok(match result {
        CoreResult::Success(success) => CompositionResult {
            supergraph_sdl: Some(success.supergraph_sdl().to_string()),
            errors: None,
        },
        CoreResult::Failure(failure) => CompositionResult {
            supergraph_sdl: None,
            errors: Some(
                failure
                    .errors()
                    .iter()
                    .map(|e| CompositionError {
                        message: e.message.clone(),
                        code: e.code().map(str::to_string),
                    })
                    .collect(),
            ),
        },
    })
}
