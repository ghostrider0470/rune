//! Clap-based CLI definition with all subcommands.

use clap::{Parser, Subcommand};

/// Rune — the operator CLI for managing the Rune AI runtime.
#[derive(Debug, Parser)]
#[command(name = "rune", version, about = "Rune AI runtime operator CLI")]
pub struct Cli {
    /// Output as JSON instead of human-readable text.
    #[arg(long, global = true)]
    pub json: bool,

    /// Gateway base URL (default: http://127.0.0.1:8787).
    #[arg(long, global = true, env = "RUNE_GATEWAY_URL", default_value = "http://127.0.0.1:8787")]
    pub gateway_url: String,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Manage the gateway daemon.
    Gateway {
        #[command(subcommand)]
        action: GatewayAction,
    },
    /// Query gateway status.
    Status,
    /// Run a health check against the gateway.
    Health,
    /// Run diagnostic checks (config, connectivity, etc.).
    Doctor,
    /// Manage sessions.
    Sessions {
        #[command(subcommand)]
        action: SessionsAction,
    },
    /// Manage configuration.
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

#[derive(Debug, Subcommand)]
pub enum GatewayAction {
    /// Start the gateway daemon.
    Start,
    /// Stop the gateway daemon.
    Stop,
}

#[derive(Debug, Subcommand)]
pub enum SessionsAction {
    /// List all sessions.
    List,
    /// Show details for a specific session.
    Show {
        /// Session ID to inspect.
        id: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum ConfigAction {
    /// Dump the resolved configuration.
    Show,
    /// Validate the configuration file.
    Validate {
        /// Path to config file (default: rune.toml).
        #[arg(short, long)]
        file: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn parse_status() {
        let cli = Cli::try_parse_from(["rune", "status"]).unwrap();
        assert!(matches!(cli.command, Command::Status));
        assert!(!cli.json);
    }

    #[test]
    fn parse_health() {
        let cli = Cli::try_parse_from(["rune", "health"]).unwrap();
        assert!(matches!(cli.command, Command::Health));
    }

    #[test]
    fn parse_doctor() {
        let cli = Cli::try_parse_from(["rune", "doctor"]).unwrap();
        assert!(matches!(cli.command, Command::Doctor));
    }

    #[test]
    fn parse_json_flag() {
        let cli = Cli::try_parse_from(["rune", "--json", "status"]).unwrap();
        assert!(cli.json);
    }

    #[test]
    fn parse_gateway_start() {
        let cli = Cli::try_parse_from(["rune", "gateway", "start"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Gateway {
                action: GatewayAction::Start
            }
        ));
    }

    #[test]
    fn parse_gateway_stop() {
        let cli = Cli::try_parse_from(["rune", "gateway", "stop"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Gateway {
                action: GatewayAction::Stop
            }
        ));
    }

    #[test]
    fn parse_sessions_list() {
        let cli = Cli::try_parse_from(["rune", "sessions", "list"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Sessions {
                action: SessionsAction::List
            }
        ));
    }

    #[test]
    fn parse_sessions_show() {
        let cli = Cli::try_parse_from(["rune", "sessions", "show", "abc-123"]).unwrap();
        match &cli.command {
            Command::Sessions {
                action: SessionsAction::Show { id },
            } => assert_eq!(id, "abc-123"),
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_config_show() {
        let cli = Cli::try_parse_from(["rune", "config", "show"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Config {
                action: ConfigAction::Show
            }
        ));
    }

    #[test]
    fn parse_config_validate() {
        let cli = Cli::try_parse_from(["rune", "config", "validate"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Config {
                action: ConfigAction::Validate { file: None }
            }
        ));
    }

    #[test]
    fn parse_config_validate_with_file() {
        let cli = Cli::try_parse_from(["rune", "config", "validate", "-f", "custom.toml"]).unwrap();
        match &cli.command {
            Command::Config {
                action: ConfigAction::Validate { file },
            } => assert_eq!(file.as_deref(), Some("custom.toml")),
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_custom_gateway_url() {
        let cli = Cli::try_parse_from(["rune", "--gateway-url", "http://localhost:9999", "health"]).unwrap();
        assert_eq!(cli.gateway_url, "http://localhost:9999");
    }

    #[test]
    fn missing_subcommand_is_error() {
        assert!(Cli::try_parse_from(["rune"]).is_err());
    }
}
