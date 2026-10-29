use actix_web::{
    dev::{Service, ServiceRequest, ServiceResponse, Transform},
    Error,
};
use actix_web::{App, HttpServer};
use ferrumec_perms::{PermissionSet, Permissions};
use futures_util::future::{ok, LocalBoxFuture, Ready};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Instant;

use crate::observability::{Observability, record_request};
use ferrumec::{Infra, Module};

/// Initializes observability and runs an Actix Web server hosting `modules`.
///
/// Each `(namespace, module)` pair is registered with [`Module::configure`]
/// on every worker. Requests are recorded through [`record_request`] (count
/// and duration by method, route and status).
///
/// This is an opinionated bootstrap: it reports telemetry under the service
/// name `"mains"`, binds to `HOST:PORT` (defaults `0.0.0.0:8080`), and panics
/// if observability cannot be initialized. Telemetry is flushed after the
/// server stops. For other settings, build your own `HttpServer` and use
/// [`Observability`] and [`record_request`] directly.
pub async fn launch(
    modules: Vec<(&'static str, Arc<dyn Module>)>,
    infra: impl Infra,
) -> std::io::Result<()> {
    // ---------------------------------------------------------
    // Observability
    // ---------------------------------------------------------

    let telemetry = Observability::init("mains", env!("CARGO_PKG_VERSION"))
        .expect("failed to initialize observability");

    tracing::info!(
        "Observability initialized: logs=VictoriaLogs, metrics=VictoriaMetrics"
    );

    // ---------------------------------------------------------
    // HTTP server
    // ---------------------------------------------------------

    let modules = Arc::new(modules);

    let permission_set = PermissionSet::from_file("permissions.json").unwrap_or_else(|e| {
        tracing::error!("failed to load permissions.json: {e}");
        panic!("failed to load permissions.json: {e}");
    });

    let permission_middleware = Permissions::new(permission_set, infra.clone());

    let host = std::env::var("HOST").unwrap_or("0.0.0.0".to_string());
    let port = std::env::var("PORT")
        .unwrap_or("8080".to_string())
        .parse::<u16>()
        .expect("invalid value for PORT");

    let server = HttpServer::new({
        let modules = Arc::clone(&modules);

        move || {
            let app = App::new()
                .wrap(permission_middleware.clone())
                .wrap(MetricsMiddleware);

            // API routes first, so they take priority over the static catch-all.
            let mut api = actix_web::web::scope("/api");
            for (namespace, module) in modules.iter() {
                api = api.configure(|cfg| {
                    module.clone().configure(cfg, namespace);
                });
            }
            api = api.configure(|cfg| infra.clone().configure(cfg, "health"));

            let mut app = app.service(api);

            // Static frontend last: it matches everything under "/".
            if let Ok(frontend) = std::env::var("FRONTEND") {
                app = app.default_service(
                    actix_files::Files::new("/", frontend).index_file("index.html"),
                );
            }

            app
        }
    })
    .bind((host, port))?
    .run();

    // ---------------------------------------------------------
    // Wait for Actix to stop.
    // ---------------------------------------------------------

    let result = server.await;

    // Flush/shutdown telemetry providers after the HTTP server
    // has stopped accepting requests.
    telemetry.shutdown();
    result
}

/// Actix middleware that records request count and duration metrics.
pub struct MetricsMiddleware;

impl<S, B> Transform<S, ServiceRequest> for MetricsMiddleware
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Transform = MetricsMiddlewareService<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ok(MetricsMiddlewareService { service })
    }
}

pub struct MetricsMiddlewareService<S> {
    service: S,
}

impl<S, B> Service<ServiceRequest> for MetricsMiddlewareService<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(cx)
    }

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let start = Instant::now();
        let method = req.method().to_string();
        // Fall back path for when matching has not run yet (outer middleware).
        let path = req.path().to_string();

        let fut = self.service.call(req);

        Box::pin(async move {
            let res = fut.await?;
            // Prefer the matched route pattern (available after routing); fall back to path.
            let route = res
                .request()
                .match_pattern()
                .unwrap_or(path);
            let status = res.status().as_u16();
            let duration = start.elapsed();
            record_request(&method, &route, status, duration);
            Ok(res)
        })
    }
}

