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
pub use server::{start, GatewayHandle, Services};
pub use state::AppState;
pub use supervisor::BackgroundSupervisor;
