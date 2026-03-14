#![doc = "Daemon, HTTP/WS server, auth middleware, and service wiring for Rune."]

mod auth;
mod error;
mod logging;
mod routes;
mod server;
mod state;
mod supervisor;
mod ws;

pub use error::GatewayError;
pub use logging::init_logging;
pub use server::{GatewayHandle, Services, build_router, start};
pub use state::{AppState, SessionEvent};
pub use supervisor::BackgroundSupervisor;
pub(crate) use supervisor::{SupervisorDeps, run_job_lifecycle};
