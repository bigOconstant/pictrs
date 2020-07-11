use actix_web::{
    dev::{Service, ServiceRequest, Transform},
    http::StatusCode,
    HttpResponse, ResponseError,
};
use futures::future::{ok, LocalBoxFuture, Ready};
use std::task::{Context, Poll};
use tracing_futures::{Instrument, Instrumented};
use uuid::Uuid;

pub(crate) struct Tracing;

pub(crate) struct TracingMiddleware<S> {
    inner: S,
}

pub(crate) struct Internal(pub(crate) Option<String>);
pub(crate) struct InternalMiddleware<S>(Option<String>, S);
#[derive(Clone, Debug, thiserror::Error)]
#[error("Invalid API Key")]
struct ApiError;

impl ResponseError for ApiError {
    fn status_code(&self) -> StatusCode {
        StatusCode::UNAUTHORIZED
    }

    fn error_response(&self) -> HttpResponse {
        HttpResponse::build(self.status_code()).json(serde_json::json!({ "msg": self.to_string() }))
    }
}

impl<S> Transform<S> for Tracing
where
    S: Service,
    S::Future: 'static,
{
    type Request = S::Request;
    type Response = S::Response;
    type Error = S::Error;
    type InitError = ();
    type Transform = TracingMiddleware<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ok(TracingMiddleware { inner: service })
    }
}

impl<S> Service for TracingMiddleware<S>
where
    S: Service,
    S::Future: 'static,
{
    type Request = S::Request;
    type Response = S::Response;
    type Error = S::Error;
    type Future = Instrumented<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: S::Request) -> Self::Future {
        let uuid = Uuid::new_v4();

        self.inner
            .call(req)
            .instrument(tracing::info_span!("request", ?uuid))
    }
}

impl<S> Transform<S> for Internal
where
    S: Service<Request = ServiceRequest, Error = actix_web::Error>,
    S::Future: 'static,
{
    type Request = S::Request;
    type Response = S::Response;
    type Error = S::Error;
    type InitError = ();
    type Transform = InternalMiddleware<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ok(InternalMiddleware(self.0.clone(), service))
    }
}

impl<S> Service for InternalMiddleware<S>
where
    S: Service<Request = ServiceRequest, Error = actix_web::Error>,
    S::Future: 'static,
{
    type Request = S::Request;
    type Response = S::Response;
    type Error = S::Error;
    type Future = LocalBoxFuture<'static, Result<S::Response, S::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.1.poll_ready(cx)
    }

    fn call(&mut self, req: S::Request) -> Self::Future {
        if let Some(value) = req.headers().get("x-api-token") {
            if value.to_str().is_ok() && value.to_str().ok() == self.0.as_ref().map(|s| s.as_str())
            {
                let fut = self.1.call(req);
                return Box::pin(async move { fut.await });
            }
        }

        Box::pin(async move { Err(ApiError.into()) })
    }
}
