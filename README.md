# Ferrumec Main

`ferrumec-main` is a Rust library for bootstrapping an Actix Web server around [Ferrumec](https://crates.io/) modules. It includes permission middleware, optional static frontend hosting, and OpenTelemetry-based logs, metrics, and traces.

> **Project status:** The supplied `src/main.rs` is still the default “Hello, world!” binary. The server bootstrap is exposed as a library function from `src/lib.rs`; the current binary does not call it.

## Features

- Registers Ferrumec `Module` implementations under the `/api` scope.
- Adds the infrastructure configuration under `/api` using the namespace `health`.
- Loads permissions from `permissions.json` and applies `ferrumec-perms` middleware.
- Uses Actix request tracing through `tracing-actix-web`.
- Records completed HTTP request counts and durations with method, route, and response status attributes.
- Exports OpenTelemetry logs, metrics, and traces over OTLP/HTTP.
- Can serve a built frontend from a directory specified by `FRONTEND`.

## Requirements

- A Rust toolchain compatible with the Rust 2024 edition.
- A `permissions.json` file in the process working directory when using `launch`.
- OTLP-compatible telemetry endpoints. The default endpoints target a local VictoriaLogs / VictoriaMetrics / VictoriaTraces setup.
- If serving a frontend, a directory containing `index.html`.

## Installation

This project is currently configured as a Cargo package. To use it from another Rust project, add a path dependency:

```toml
[dependencies]
ferrumec-main = { path = "../ferrumec-main" }
```

Adjust the path to match your repository layout. The package manifest currently does not declare a `launch` feature, despite the source documentation mentioning one; the `launch` module is exported unconditionally.

## Configuration

The server reads these environment variables:

| Variable | Default | Purpose |
| --- | --- | --- |
| `HOST` | `0.0.0.0` | Address the HTTP server binds to. |
| `PORT` | `8080` | TCP port to bind to. Must parse as a `u16`. |
| `FRONTEND` | Unset | Directory served as the static frontend. When set, files are served from `/` and `index.html` is used as the index file. |
| `RUST_LOG` | `info` | Filter for tracing output. Uses the `tracing_subscriber` environment filter. |
| `OTEL_LOGS_ENDPOINT` | `http://127.0.0.1:9428/insert/opentelemetry/v1/logs` | OTLP/HTTP endpoint for logs. |
| `OTEL_METRICS_ENDPOINT` | `http://127.0.0.1:8428/opentelemetry/v1/metrics` | OTLP/HTTP endpoint for metrics. |
| `OTEL_TRACES_ENDPOINT` | `http://127.0.0.1:10428/insert/opentelemetry/v1/traces` | OTLP/HTTP endpoint for traces. |

The telemetry endpoints are configured as complete endpoint URLs. Ensure that the selected backends accept OTLP/HTTP requests at those paths.

## Permissions

`launch` loads `permissions.json` from the current working directory:

```rust
PermissionSet::from_file("permissions.json")
```

The expected file format is defined by `ferrumec-perms`; consult that crate's documentation or an existing valid project configuration for the exact schema. If the file cannot be loaded, startup logs an error and panics. Make sure the file is deployed alongside the process and that its permissions are appropriate for the runtime user.

## Registering modules and starting the server

The public entry point is:

```rust
pub async fn launch(
    modules: Vec<(&'static str, Arc<dyn Module>)>,
    infra: impl Infra,
) -> std::io::Result<()>
```

Each module is registered beneath `/api` using its associated namespace. The infrastructure implementation is also configured beneath `/api` with the namespace `health`.

A minimal integration sketch looks like this; replace `YourInfra` and `YourModule` with concrete types from your application and Ferrumec setup:

```rust
use std::sync::Arc;
use ferrumec::{Infra, Module};
use ferrumec_main::launch;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let modules: Vec<(&'static str, Arc<dyn Module>)> = vec![
        // ("example", Arc::new(YourModule::new())),
    ];

    // Supply your concrete infrastructure implementation:
    // let infra = YourInfra::new();
    // launch(modules, infra).await

    todo!("construct modules and infrastructure, then call launch")
}
```

This snippet is illustrative, not a standalone compiling example: the concrete constructors and trait implementations depend on the application.

### Routing behavior

- API modules and the infrastructure's `health` configuration are mounted under `/api`.
- API routes are configured before the static frontend catch-all.
- If `FRONTEND` is set, unmatched requests can be handled by static file serving from that directory.
- The default bind address is `0.0.0.0:8080`, which makes the server listen on all interfaces in typical container environments.

## Observability

`Observability::init(service_name, version)` installs global OpenTelemetry providers and a `tracing` subscriber. It configures:

- A logs provider with a batch OTLP exporter.
- A metrics provider with a periodic OTLP exporter.
- A traces provider with a batch OTLP exporter.
- Console formatting and the `RUST_LOG` filter.
- Resource attributes including `service.name` and `service.version`.

The bootstrap function initializes telemetry with service name `mains` and the package version from Cargo metadata.

### Metrics

For each completed HTTP request, `record_request` records:

- `http.server.request.count` — counter of completed requests.
- `http.server.request.duration` — histogram of request duration in seconds.

Both instruments include these attributes:

- `http.request.method`
- `http.route` (the matched route pattern, or `<unmatched>`)
- `http.response.status_code`

The metrics are recorded after the downstream service returns a response. Requests that fail before a response is produced may not be recorded by this middleware.

### Shutdown

After the Actix server stops, `launch` calls `Observability::shutdown()` to shut down the log, trace, and metric providers. Shutdown failures are written to standard error.

## Build and run

Build the package:

```sh
cargo build
```

Run the current binary:

```sh
cargo run
```

**Note:** At present, `cargo run` executes the placeholder `main` function and prints `Hello, world!`; it does not start the Actix server. To run the server, add application-specific module and infrastructure implementations and call `ferrumec_main::launch(...)` from the application entry point.

## Important implementation notes

- `launch` panics if telemetry initialization fails, if `PORT` is invalid, or if `permissions.json` cannot be loaded.
- The telemetry providers use global OpenTelemetry state, and the request instruments are initialized once per process. Calling `Observability::init` more than once is not supported.
- `FRONTEND` is optional. Without it, this crate does not configure a static frontend service.
- The current source imports `actix_files`, `actix_web`, Ferrumec, permission middleware, and OpenTelemetry dependencies directly; verify your deployment's configuration and backend compatibility before production use.

## License

No license file was included in the supplied project archive. Add a `LICENSE` file and update this section before publishing or redistributing the project.
