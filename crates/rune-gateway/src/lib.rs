#![doc = "Daemon, HTTP/WS server, auth middleware, and service wiring for Rune."]

mod a2ui;
mod auth;
mod error;
pub mod events;
pub mod logging;
pub mod ms365;
pub mod pairing;
mod routes;
mod server;
mod state;
mod supervisor;
pub mod tool_execution_repo;
mod webchat;
pub mod ws;
pub mod ws_rpc;

pub use error::GatewayError;
pub use events::{
    ApprovalEvent, ProcessEvent, RuntimeEvent, ToolEvent, TurnEvent, UsageSummary,
    broadcast_runtime_event,
};
pub use logging::init_logging;
pub use server::{GatewayHandle, Services, build_router, start};
pub use state::{AppState, SessionEvent, WebChatRateLimiter};
pub use supervisor::BackgroundSupervisor;
pub(crate) use supervisor::{SupervisorDeps, run_job_lifecycle};

pub fn telegram_adapter_from_token(token: &str) -> rune_channels::TelegramAdapter {
    rune_channels::TelegramAdapter::new(token)
}
