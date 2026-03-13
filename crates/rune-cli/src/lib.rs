#![doc = "Operator CLI for Rune: subcommands, output formatting, and gateway client."]

pub mod cli;
pub mod client;
pub mod output;

use anyhow::Result;

pub use cli::Cli;
use cli::{Command, ConfigAction, GatewayAction, SessionsAction};
use client::{show_config, validate_config, GatewayClient};
use output::{render, OutputFormat};

/// Execute the parsed CLI command against the configured gateway and print output.
pub async fn run(cli: Cli) -> Result<()> {
    let format = OutputFormat::from_json_flag(cli.json);
    let client = GatewayClient::new(&cli.gateway_url);

    match cli.command {
        Command::Gateway { action } => match action {
            GatewayAction::Status => {
                let result = client.gateway_status().await?;
                println!("{}", render(&result, format));
            }
            GatewayAction::Health => {
                let result = client.gateway_health().await?;
                println!("{}", render(&result, format));
            }
            GatewayAction::Start => {
                let result = client.gateway_start().await?;
                println!("{}", render(&result, format));
            }
            GatewayAction::Stop => {
                let result = client.gateway_stop().await?;
                println!("{}", render(&result, format));
            }
            GatewayAction::Restart => {
                let result = client.gateway_restart().await?;
                println!("{}", render(&result, format));
            }
        },
        Command::Status => {
            let result = client.status().await?;
            println!("{}", render(&result, format));
        }
        Command::Health => {
            let result = client.health().await?;
            println!("{}", render(&result, format));
        }
        Command::Doctor => {
            let result = client.doctor().await?;
            println!("{}", render(&result, format));
        }
        Command::Sessions { action } => match action {
            SessionsAction::List => {
                let result = client.sessions_list().await?;
                println!("{}", render(&result, format));
            }
            SessionsAction::Show { id } => {
                let result = client.sessions_show(&id).await?;
                println!("{}", render(&result, format));
            }
        },
        Command::Config { action } => match action {
            ConfigAction::Show => {
                let result = show_config()?;
                if matches!(format, OutputFormat::Json) {
                    println!("{result}");
                } else {
                    println!("Resolved configuration:\n{result}");
                }
            }
            ConfigAction::Validate { file } => {
                let result = validate_config(file.as_deref());
                println!("{}", render(&result, format));
            }
        },
    }

    Ok(())
}
