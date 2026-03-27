#![doc = "Operator CLI for Rune: subcommands, output formatting, and gateway client."]

pub mod cli;
pub mod client;
pub mod doctor;
mod logs;
pub mod memory;
pub mod output;
pub mod service;

use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Utc};
use clap::CommandFactory;
use clap_complete::{Shell, generate};
use reqwest::blocking::Client as BlockingHttpClient;
use sha2::{Digest, Sha256};
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command as StdCommand};
use std::time::SystemTime;
use toml_edit::{DocumentMut, Item, Table, Value};

#[cfg(test)]
pub(crate) fn test_env_lock() -> &'static std::sync::Mutex<()> {
    use std::sync::{Mutex, OnceLock};

    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

pub use cli::Cli;
use cli::{
    AcpAction, AgentAction, AgentsAction, ApprovalsAction, BackupAction, ChannelsAction, Command,
    CompletionAction, CompletionShell, ConfigAction, CronAction, CronDeliveryMode, DoctorAction,
    GatewayAction, GatewayConfigAction, GatewayRuntimeAction, GatewayRuntimeHeartbeatAction,
    HooksAction, LogsAction, LogsArgs, MemoryAction, MessageAction, MessageTagAction,
    MessageThreadAction, MessageVoiceAction, ModelsAction, Ms365Action, Ms365AuthAction,
    Ms365CalendarAction, Ms365FilesAction, Ms365MailAction, Ms365PlannerAction, Ms365SitesAction,
    Ms365TeamsAction, Ms365TodoAction, Ms365UsersAction, PluginsAction, ProcessAction,
    RemindersAction, SandboxAction, SecretsAction, SecurityAction, ServiceAction, ServiceTarget,
    SessionsAction, SkillsAction, SpellsAction, SystemAction, SystemEventAction,
    SystemHeartbeatAction, UpdateAction,
};
use client::{
    GatewayClient, config_file, config_get, config_set, config_unset, show_config, validate_config,
};
use output::{
    ChannelCapabilitiesResponse, ChannelDetail, ChannelListResponse, ChannelLogFile,
    ChannelLogsResponse, ChannelResolveResponse, ChannelStatusResponse, DashboardChannelsSummary,
    DashboardModelsSummary, DashboardResponse, DashboardSessionsSummary, HeartbeatPresenceResponse,
    ModelAliasDetail, ModelAliasesResponse, ModelAuthProviderDetail, ModelAuthResponse,
    ModelFallbackChainDetail, ModelFallbacksResponse, ModelListResponse, ModelProviderDetail,
    ModelScanResponse, ModelSetImageResponse, ModelSetResponse, ModelStatusResponse, OutputFormat,
    SpellSearchResponse, TemplateListResponse, TemplateStartResponse, TemplateSummary, render,
};
use service::{ServiceInstallOptions, install_service_definition, print_service_definition};

/// Initialize a workspace directory with default files.
fn load_config() -> rune_config::AppConfig {
    rune_config::AppConfig::load(None::<&std::path::Path>).unwrap_or_default()
}

fn discover_local_config_path() -> std::path::PathBuf {
    if let Some(config_path) = std::env::var_os("RUNE_CONFIG") {
        return std::path::PathBuf::from(config_path);
    }

    let profile = std::env::var("RUNE_PROFILE")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    match profile.as_deref() {
        Some("dev") => std::path::PathBuf::from("config.dev.toml"),
        Some(profile) => std::path::PathBuf::from(format!("config.{profile}.toml")),
        None => std::path::PathBuf::from("config.toml"),
    }
}

fn apply_global_cli_environment(cli: &Cli) {
    if cli.dev && cli.profile.is_none() && std::env::var_os("RUNE_PROFILE").is_none() {
        unsafe {
            std::env::set_var("RUNE_PROFILE", "dev");
        }
    }

    if let Some(profile) = &cli.profile {
        unsafe {
            std::env::set_var("RUNE_PROFILE", profile);
        }
    }

    if let Some(level) = &cli.log_level {
        unsafe {
            std::env::set_var("RUNE_LOG_LEVEL", level);
        }
        if std::env::var_os("RUST_LOG").is_none() {
            unsafe {
                std::env::set_var("RUST_LOG", level);
            }
        }
    }

    if cli.no_color {
        unsafe {
            std::env::set_var("NO_COLOR", "1");
        }
        clap::builder::styling::Styles::plain();
    }

    // Trusted-environment bypass flags (issue #64): set env vars so that
    // subsequent config loads (e.g. `rune doctor`) resolve the override
    // without requiring a TOML edit.
    if cli.yolo {
        unsafe {
            std::env::set_var("RUNE_APPROVAL__MODE", "yolo");
        }
    }
    if cli.no_sandbox {
        unsafe {
            std::env::set_var("RUNE_SECURITY__SANDBOX", "false");
        }
    }
}

fn run_gateway_foreground(yolo: bool, no_sandbox: bool) -> Result<()> {
    let mut args = Vec::new();
    if let Some(config_path) = std::env::var_os("RUNE_CONFIG") {
        args.push("--config".to_string());
        args.push(config_path.to_string_lossy().into_owned());
    }
    if yolo {
        args.push("--yolo".to_string());
    }
    if no_sandbox {
        args.push("--no-sandbox".to_string());
    }

    let status = StdCommand::new("rune-gateway")
        .args(&args)
        .status()
        .context("failed to start `rune-gateway`; ensure the binary is installed and on PATH")?;

    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("`rune-gateway` exited with status {status}");
    }
}

fn completion_shell(shell: CompletionShell) -> Shell {
    match shell {
        CompletionShell::Bash => Shell::Bash,
        CompletionShell::Elvish => Shell::Elvish,
        CompletionShell::Fish => Shell::Fish,
        CompletionShell::PowerShell => Shell::PowerShell,
        CompletionShell::Zsh => Shell::Zsh,
    }
}

fn print_completion(shell: CompletionShell) -> Result<()> {
    let mut command = Cli::command();
    generate(
        completion_shell(shell),
        &mut command,
        "rune",
        &mut std::io::stdout(),
    );
    Ok(())
}

fn prompt_text(label: &str, default: Option<&str>) -> Result<String> {
    let mut stderr = std::io::stderr();
    match default {
        Some(default) if !default.is_empty() => write!(stderr, "{label} [{default}]: ")?,
        _ => write!(stderr, "{label}: ")?,
    }
    stderr.flush().ok();

    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    let trimmed = input.trim();
    if trimmed.is_empty() {
        Ok(default.unwrap_or_default().to_string())
    } else {
        Ok(trimmed.to_string())
    }
}

fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    Ok(())
}

fn ensure_table<'a>(doc: &'a mut DocumentMut, section: &str) -> Result<&'a mut Table> {
    if !doc.contains_key(section) || !doc[section].is_table() {
        doc[section] = Item::Table(Table::new());
    }
    doc[section]
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("{section} must be a table"))
}

fn set_string(doc: &mut DocumentMut, section: &str, key: &str, value: &str) -> Result<()> {
    if key.is_empty() {
        doc[section] = Item::Value(Value::from(value));
        return Ok(());
    }
    ensure_table(doc, section)?[key] = Item::Value(Value::from(value));
    Ok(())
}

fn set_bool(doc: &mut DocumentMut, section: &str, key: &str, value: bool) -> Result<()> {
    if key.is_empty() {
        doc[section] = Item::Value(Value::from(value));
        return Ok(());
    }
    ensure_table(doc, section)?[key] = Item::Value(Value::from(value));
    Ok(())
}

fn set_array_strings(
    doc: &mut DocumentMut,
    section: &str,
    key: &str,
    values: &[&str],
) -> Result<()> {
    let mut arr = toml_edit::Array::default();
    for value in values {
        arr.push(*value);
    }
    ensure_table(doc, section)?[key] = Item::Value(Value::Array(arr));
    Ok(())
}

fn discover_home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn default_workspace_root() -> Result<PathBuf> {
    discover_home_dir()
        .map(|home| home.join(".rune"))
        .ok_or_else(|| anyhow::anyhow!("failed to resolve home directory for default workspace"))
}

fn resolve_workspace_path(input: &str) -> Result<PathBuf> {
    if input == "." {
        return default_workspace_root();
    }

    if let Some(stripped) = input.strip_prefix("~/") {
        if let Some(home) = discover_home_dir() {
            return Ok(home.join(stripped));
        }
    }

    if input == "~" {
        if let Some(home) = discover_home_dir() {
            return Ok(home);
        }
    }

    Ok(PathBuf::from(input))
}

fn open_config_instructions(workspace: &Path, config_path: &Path) {
    let workspace_hint = workspace.display();
    let config_hint = config_path.display();
    eprintln!("next steps:");
    eprintln!("  - config: {}", config_hint);
    eprintln!("  - workspace: {}", workspace_hint);
    eprintln!("  - start manually: rune-gateway --config {}", config_hint);
    eprintln!(
        "  - install a background service: rune service install --target systemd --name rune-gateway --workdir {} --config {} --enable --start",
        workspace_hint, config_hint
    );
    eprintln!(
        "  - macOS launchd variant: rune service install --target launchd --name rune-gateway --workdir {} --config {} --enable --start",
        workspace_hint, config_hint
    );
    eprintln!("  - verify runtime: rune --gateway-url http://127.0.0.1:8787 health");
    eprintln!("  - verify diagnostics: rune --gateway-url http://127.0.0.1:8787 doctor run");
    eprintln!("  - open chat: http://127.0.0.1:8787/webchat");
    eprintln!("  - open dashboard: http://127.0.0.1:8787/dashboard");
    eprintln!("  - legacy chat redirect: http://127.0.0.1:8787/chat");
}

fn start_gateway_process(
    workspace: &Path,
    config_path: &Path,
    repo_root_hint: Option<&Path>,
) -> Result<Child> {
    let mut candidates: Vec<(String, StdCommand)> = Vec::new();

    let mut direct = StdCommand::new("rune-gateway");
    direct
        .arg("--config")
        .arg(config_path)
        .current_dir(workspace);
    candidates.push(("rune-gateway".to_string(), direct));

    if let Ok(exe) = std::env::current_exe() {
        if let Some(bin_dir) = exe.parent() {
            let gateway_name = if cfg!(windows) {
                "rune-gateway.exe"
            } else {
                "rune-gateway"
            };
            let gateway_path = bin_dir.join(gateway_name);
            if gateway_path.exists() {
                let mut sibling = StdCommand::new(&gateway_path);
                sibling
                    .arg("--config")
                    .arg(config_path)
                    .current_dir(workspace);
                candidates.push((gateway_path.display().to_string(), sibling));
            }
        }
    }

    let mut cargo = StdCommand::new("cargo");
    cargo
        .arg("run")
        .arg("--release")
        .arg("--bin")
        .arg("rune-gateway")
        .arg("--")
        .arg("--config")
        .arg(config_path)
        .current_dir(repo_root_hint.unwrap_or(workspace));
    candidates.push(("cargo run --release --bin rune-gateway".to_string(), cargo));

    let mut last_err = None;
    for (label, mut cmd) in candidates {
        match cmd.spawn() {
            Ok(child) => return Ok(child),
            Err(err) => last_err = Some(format!("{label}: {err}")),
        }
    }

    anyhow::bail!(
        "failed to start gateway automatically ({})",
        last_err.unwrap_or_else(|| "no launch candidates".to_string())
    )
}

fn ollama_default_model() -> &'static str {
    "llama3.2"
}

fn detect_ollama() -> Option<String> {
    let host = std::env::var("OLLAMA_HOST")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "http://127.0.0.1:11434".to_string());

    let normalized = if host.contains("/v1") || host.contains("/api") {
        host.trim_end_matches('/').to_string()
    } else {
        format!("{}/api/tags", host.trim_end_matches('/'))
    };

    let probe_url = if normalized.ends_with("/api/tags") {
        normalized
    } else {
        format!("{}/api/tags", normalized.trim_end_matches('/'))
    };

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_millis(1200))
        .build()
        .ok()?;

    let response = client.get(&probe_url).send().ok()?;
    if response.status().is_success() {
        Some(host.trim_end_matches('/').to_string())
    } else {
        None
    }
}

fn normalize_provider_kind(input: &str) -> String {
    match input.trim().to_ascii_lowercase().as_str() {
        "azure-openai" => "azure".to_string(),
        value => value.to_string(),
    }
}

fn provider_default_base_url(provider: &str) -> Option<&'static str> {
    match provider {
        "openai" => Some("https://api.openai.com/v1"),
        "anthropic" => Some("https://api.anthropic.com/v1"),
        "groq" => Some("https://api.groq.com/openai/v1"),
        "mistral" => Some("https://api.mistral.ai/v1"),
        "deepseek" => Some("https://api.deepseek.com/v1"),
        "google" => Some("https://generativelanguage.googleapis.com/v1beta/openai"),
        "ollama" => Some("http://localhost:11434/v1"),
        _ => None,
    }
}

fn provider_default_model(provider: &str) -> &'static str {
    match provider {
        "anthropic" => "claude-3-7-sonnet-latest",
        "groq" => "llama-3.3-70b-versatile",
        "mistral" => "mistral-large-latest",
        "deepseek" => "deepseek-chat",
        "google" => "gemini-2.0-flash",
        "ollama" => "llama3.2",
        "azure" => "gpt-4o",
        _ => "gpt-4o-mini",
    }
}

fn provider_env_key(provider: &str) -> Option<&'static str> {
    match provider {
        "openai" => Some("OPENAI_API_KEY"),
        "anthropic" => Some("ANTHROPIC_API_KEY"),
        "groq" => Some("GROQ_API_KEY"),
        "mistral" => Some("MISTRAL_API_KEY"),
        "deepseek" => Some("DEEPSEEK_API_KEY"),
        "google" => Some("GOOGLE_API_KEY"),
        "azure" => Some("AZURE_OPENAI_API_KEY"),
        _ => None,
    }
}

fn write_wizard_config(
    workspace: &Path,
    provider: &str,
    model: &str,
    api_key: &str,
    telegram_token: Option<&str>,
    webchat: bool,
) -> Result<PathBuf> {
    let config_path = workspace.join("config.toml");
    ensure_parent_dir(&config_path)?;

    let existing = if config_path.exists() {
        std::fs::read_to_string(&config_path)
            .with_context(|| format!("failed to read {}", config_path.display()))?
    } else {
        String::new()
    };

    let mut doc = if existing.trim().is_empty() {
        DocumentMut::new()
    } else {
        existing
            .parse::<DocumentMut>()
            .with_context(|| format!("failed to parse {}", config_path.display()))?
    };

    set_string(&mut doc, "mode", "", "standalone")?;
    if doc["mode"].is_table() {
        doc["mode"] = Item::Value(Value::from("standalone"));
    }
    set_string(&mut doc, "gateway", "host", "127.0.0.1")?;
    doc["gateway"]["port"] = Item::Value(Value::from(8787));

    set_bool(&mut doc, "browser", "enabled", webchat)?;
    let enabled_channels: Vec<&str> = if webchat {
        if telegram_token.is_some() {
            vec!["webchat", "telegram"]
        } else {
            vec!["webchat"]
        }
    } else if telegram_token.is_some() {
        vec!["telegram"]
    } else {
        Vec::new()
    };
    set_array_strings(&mut doc, "channels", "enabled", &enabled_channels)?;

    let models_table = ensure_table(&mut doc, "models")?;
    models_table["default_model"] = Item::Value(Value::from(format!("local/{model}")));
    models_table["providers"] = Item::ArrayOfTables(Default::default());
    let arr = models_table["providers"]
        .as_array_of_tables_mut()
        .ok_or_else(|| anyhow::anyhow!("models.providers must be an array of tables"))?;
    arr.clear();
    let mut table = toml_edit::Table::new();
    table.set_implicit(true);
    table["name"] = Item::Value(Value::from("local"));
    table["kind"] = Item::Value(Value::from(provider));
    if provider == "ollama" || api_key.trim().is_empty() {
        if let Some(env_key) = provider_env_key(provider) {
            table["api_key_env"] = Item::Value(Value::from(env_key));
        }
    } else {
        table["api_key"] = Item::Value(Value::from(api_key));
    }
    if let Some(base_url) = provider_default_base_url(provider) {
        table["base_url"] = Item::Value(Value::from(base_url));
    }
    let mut models = toml_edit::Array::default();
    models.push(model);
    table["models"] = Item::Value(Value::Array(models));
    arr.push(table);

    let storage_table = ensure_table(&mut doc, "storage")?;
    storage_table["backend"] = Item::Value(Value::from("sqlite"));
    storage_table["sqlite_path"] = Item::Value(Value::from("state/rune.db"));

    let ui_table = ensure_table(&mut doc, "ui")?;
    ui_table["enabled"] = Item::Value(Value::from(true));

    if let Some(token) = telegram_token.filter(|value| !value.trim().is_empty()) {
        let channels_table = ensure_table(&mut doc, "channels")?;
        if !channels_table.contains_key("telegram") || !channels_table["telegram"].is_table() {
            channels_table["telegram"] = Item::Table(Table::new());
        }
        let telegram = channels_table["telegram"]
            .as_table_mut()
            .ok_or_else(|| anyhow::anyhow!("channels.telegram must be a table"))?;
        telegram["bot_token"] = Item::Value(Value::from(token));
    }

    std::fs::create_dir_all(workspace.join("state"))
        .with_context(|| format!("failed to create {}", workspace.join("state").display()))?;

    std::fs::write(&config_path, doc.to_string())
        .with_context(|| format!("failed to write {}", config_path.display()))?;
    Ok(config_path)
}

fn browser_launch_url(
    gateway_url: &str,
    chat_path: &str,
    provider: &str,
    api_key: &str,
    session_token: Option<&str>,
) -> String {
    let mut params: Vec<String> = Vec::new();
    if !api_key.trim().is_empty() && provider != "ollama" {
        let encoded = urlencoding::encode(api_key);
        params.push(format!("api_key={encoded}"));
        params.push(format!("auth={encoded}"));
    }

    if let Some(token) = session_token
        .map(str::trim)
        .filter(|token| !token.is_empty())
    {
        let encoded = urlencoding::encode(token);
        params.push(format!("session_token={encoded}"));
    }

    if params.is_empty() {
        format!("{gateway_url}{chat_path}")
    } else {
        format!("{gateway_url}{chat_path}?{}", params.join("&"))
    }
}

fn open_url_in_browser(url: &str) -> Result<()> {
    if cfg!(target_os = "linux")
        && std::env::var_os("DISPLAY").is_none()
        && std::env::var_os("WAYLAND_DISPLAY").is_none()
    {
        anyhow::bail!(
            "no graphical browser session detected; rerun with --no-open or open {} manually",
            url
        );
    }

    let candidates: &[(&str, &[&str])] = if cfg!(target_os = "macos") {
        &[("open", &[])]
    } else if cfg!(target_os = "windows") {
        &[("cmd", &["/C", "start", ""])]
    } else {
        &[("xdg-open", &[]), ("gio", &["open"])]
    };

    let mut failures = Vec::new();
    for (program, args) in candidates {
        match StdCommand::new(program).args(*args).arg(url).status() {
            Ok(status) if status.success() => return Ok(()),
            Ok(status) => failures.push(format!("{program} exited with status {status}")),
            Err(err) => failures.push(format!("{program}: {err}")),
        }
    }

    anyhow::bail!(
        "failed to open browser for {} ({})",
        url,
        failures.join("; ")
    )
}

struct InitWizardOptions<'a> {
    path: &'a str,
    api_key: Option<String>,
    provider: Option<String>,
    model: Option<String>,
    telegram_token: Option<String>,
    webchat: bool,
    start: bool,
    open: bool,
    print_url: bool,
    non_interactive: bool,
    install_service: bool,
    service_target: ServiceTarget,
    service_name: &'a str,
    service_enable: bool,
    service_start: bool,
    print_next_steps: bool,
}

async fn run_init_wizard(options: InitWizardOptions<'_>) -> Result<()> {
    let workspace = resolve_workspace_path(options.path)?;
    init_workspace(&workspace, None, options.non_interactive).await?;

    let interactive = std::io::stdin().is_terminal() && !options.non_interactive;
    let detected_ollama = detect_ollama();

    let mut webchat = options.webchat;
    if interactive {
        let default_webchat = if webchat { "Y" } else { "N" };
        let enable_webchat = prompt_text(
            "Enable browser WebChat channel? (Y/n)",
            Some(default_webchat),
        )?;
        webchat = !matches!(
            enable_webchat.trim().to_ascii_lowercase().as_str(),
            "n" | "no"
        );
    }

    let provider = match options.provider {
        Some(value) => normalize_provider_kind(&value),
        None if interactive => {
            let default_provider = if detected_ollama.is_some() {
                "ollama"
            } else {
                "openai"
            };
            normalize_provider_kind(&prompt_text("Provider", Some(default_provider))?)
        }
        None if detected_ollama.is_some() => "ollama".to_string(),
        None => "openai".to_string(),
    };

    let model_default = if provider == "ollama" {
        ollama_default_model()
    } else {
        provider_default_model(&provider)
    };
    let model = match options.model {
        Some(value) => value,
        None if interactive => prompt_text("Model", Some(model_default))?,
        None => model_default.to_string(),
    };

    let telegram_token = match options.telegram_token {
        Some(value) if !value.trim().is_empty() => Some(value),
        _ if interactive => {
            let enable = prompt_text("Enable Telegram channel? (y/N)", Some("N"))?;
            if matches!(enable.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
                let token = prompt_text("Telegram bot token", None)?;
                if token.trim().is_empty() {
                    None
                } else {
                    Some(token)
                }
            } else {
                None
            }
        }
        _ => None,
    };

    let provider_env_key = provider_env_key(&provider);
    let api_key = match options.api_key {
        Some(value) => value,
        None if provider == "ollama" => String::new(),
        None => {
            let env_key = provider_env_key.and_then(|key| std::env::var(key).ok());
            match (env_key, interactive, provider_env_key) {
                (Some(value), _, _) => value,
                (None, true, Some(env_name)) => {
                    prompt_text(&format!("API key ({env_name})"), None)?
                }
                (None, true, None) => prompt_text("API key", None)?,
                (None, false, Some(env_name)) => anyhow::bail!(
                    "missing API key for provider `{provider}`; pass --api-key or set {env_name}"
                ),
                (None, false, None) => {
                    anyhow::bail!("missing API key for provider `{provider}`; pass --api-key")
                }
            }
        }
    };

    let config_path = write_wizard_config(
        &workspace,
        &provider,
        &model,
        &api_key,
        telegram_token.as_deref(),
        webchat,
    )?;
    println!("✓ Wrote {}", config_path.display());
    if provider == "ollama" {
        if let Some(host) = detected_ollama {
            println!("✓ Auto-detected Ollama at {host}");
        }
    }

    let host = "127.0.0.1";
    let port = 8787u16;
    let gateway_url = format!("http://{host}:{port}");
    let chat_path = if webchat { "/webchat" } else { "/chat" };
    let session_token = if webchat {
        Some(uuid::Uuid::new_v4().to_string())
    } else {
        None
    };
    let url = browser_launch_url(
        &gateway_url,
        chat_path,
        &provider,
        &api_key,
        session_token.as_deref(),
    );
    let should_start_service = options.install_service && options.service_start;

    if options.install_service {
        let service = install_service_definition(ServiceInstallOptions {
            target: options.service_target,
            name: options.service_name.to_string(),
            workdir: workspace.clone(),
            config: Some(config_path.display().to_string()),
            gateway_url: Some(gateway_url.clone()),
            yolo: false,
            no_sandbox: false,
            output: None,
            enable: options.service_enable,
            start: options.service_start,
            auto_bootstrap: true,
        })?;
        if let Some(path) = service.output_path {
            println!("✓ Installed {} service at {}", service.target, path);
        } else {
            println!("✓ Installed {} service", service.target);
        }
    }

    if options.start && !should_start_service {
        let child = start_gateway_process(
            &workspace,
            &config_path,
            Some(
                &std::env::current_dir()
                    .context("failed to resolve current directory for cargo fallback")?,
            ),
        )?;
        println!("✓ Started gateway (pid {})", child.id());
        wait_for_gateway_ready(&gateway_url)?;
        println!("✓ Gateway ready at {gateway_url}");
    }

    if options.print_next_steps {
        open_config_instructions(&workspace, &config_path);
    }

    if options.print_next_steps || options.print_url {
        println!("→ Chat URL: {url}");
    }

    if options.open {
        if options.start || should_start_service {
            open_url_in_browser(&url)?;
            println!("✓ Opened {url}");
        } else {
            println!("→ Chat URL: {url}");
        }
    }

    Ok(())
}

#[derive(serde::Deserialize)]
struct GitHubReleaseAsset {
    name: String,
    browser_download_url: String,
}

#[derive(serde::Deserialize)]
struct GitHubRelease {
    tag_name: String,
    assets: Vec<GitHubReleaseAsset>,
}

#[derive(Debug)]
struct ResolvedUpdateAsset {
    version: String,
    asset_name: String,
    download_url: String,
    checksum_url: String,
}

fn apply_self_update(
    repo: &str,
    version: Option<&str>,
    binary_path: Option<&str>,
) -> Result<output::UpdateApplyResponse> {
    let current_binary = binary_path
        .map(PathBuf::from)
        .map(Ok)
        .unwrap_or_else(resolve_current_executable_path)?;
    let binary_name = current_binary
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| {
            anyhow!(
                "could not determine target binary name from {}",
                current_binary.display()
            )
        })?
        .to_string();
    let client = BlockingHttpClient::builder()
        .user_agent(format!("rune-cli/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .context("build update HTTP client")?;
    let release = fetch_release_metadata(&client, repo, version)?;
    let asset = resolve_update_asset(&release, &binary_name)?;
    let install_dir = current_binary.parent().ok_or_else(|| {
        anyhow!(
            "binary path {} has no parent directory",
            current_binary.display()
        )
    })?;
    std::fs::create_dir_all(install_dir)
        .with_context(|| format!("create install directory {}", install_dir.display()))?;

    let temp_dir = tempfile::tempdir().context("create update staging dir")?;
    let staged_binary = temp_dir.path().join(&asset.asset_name);
    let checksum_file = temp_dir.path().join("SHA256SUMS");

    download_to_path(&client, &asset.download_url, &staged_binary)?;
    download_to_path(&client, &asset.checksum_url, &checksum_file)?;
    verify_sha256(&staged_binary, &checksum_file, &asset.asset_name)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&staged_binary)
            .with_context(|| format!("stat {}", staged_binary.display()))?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&staged_binary, perms)
            .with_context(|| format!("chmod {}", staged_binary.display()))?;
    }

    let backup_path = current_binary.with_extension("bak");
    if backup_path.exists() {
        std::fs::remove_file(&backup_path)
            .with_context(|| format!("remove old backup {}", backup_path.display()))?;
    }
    if current_binary.exists() {
        std::fs::rename(&current_binary, &backup_path).with_context(|| {
            format!(
                "move current binary {} to backup {}",
                current_binary.display(),
                backup_path.display()
            )
        })?;
    }

    if let Err(error) = std::fs::rename(&staged_binary, &current_binary) {
        if backup_path.exists() {
            let _ = std::fs::rename(&backup_path, &current_binary);
        }
        return Err(error).with_context(|| {
            format!(
                "replace binary {} with staged update {}",
                current_binary.display(),
                staged_binary.display()
            )
        });
    }

    Ok(output::UpdateApplyResponse {
        success: true,
        detail: format!(
            "installed {} from GitHub release {}",
            asset.asset_name, asset.version
        ),
        previous_version: Some(env!("CARGO_PKG_VERSION").to_string()),
        installed_version: Some(asset.version),
        binary_path: Some(current_binary.display().to_string()),
        asset_name: Some(asset.asset_name),
    })
}

fn resolve_current_executable_path() -> Result<PathBuf> {
    std::env::current_exe().context("resolve current executable path")
}

fn fetch_release_metadata(
    client: &BlockingHttpClient,
    repo: &str,
    version: Option<&str>,
) -> Result<GitHubRelease> {
    let url = match version {
        Some(version) => format!("https://api.github.com/repos/{repo}/releases/tags/{version}"),
        None => format!("https://api.github.com/repos/{repo}/releases/latest"),
    };
    let response = client
        .get(&url)
        .send()
        .with_context(|| format!("fetch release metadata from {url}"))?;
    if !response.status().is_success() {
        bail!("release metadata request failed with {}", response.status());
    }
    response.json().context("parse GitHub release metadata")
}

fn resolve_update_asset(release: &GitHubRelease, binary_name: &str) -> Result<ResolvedUpdateAsset> {
    let target = match std::env::consts::OS {
        "linux" => "linux",
        "macos" => "macos",
        "windows" => "windows",
        other => bail!("self-update is not supported on operating system {other}"),
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        other => bail!("self-update is not supported on architecture {other}"),
    };
    let expected_prefix = format!("{binary_name}-{target}-{arch}");
    let asset = release
        .assets
        .iter()
        .find(|asset| {
            asset.name == expected_prefix || asset.name == format!("{expected_prefix}.exe")
        })
        .ok_or_else(|| {
            anyhow!(
                "release {} does not contain asset {} for this platform",
                release.tag_name,
                expected_prefix
            )
        })?;
    let checksum_url = release
        .assets
        .iter()
        .find(|candidate| candidate.name == "SHA256SUMS")
        .map(|asset| asset.browser_download_url.clone())
        .ok_or_else(|| anyhow!("release {} is missing SHA256SUMS", release.tag_name))?;

    Ok(ResolvedUpdateAsset {
        version: release.tag_name.clone(),
        asset_name: asset.name.clone(),
        download_url: asset.browser_download_url.clone(),
        checksum_url,
    })
}

fn download_to_path(client: &BlockingHttpClient, url: &str, path: &Path) -> Result<()> {
    let response = client
        .get(url)
        .send()
        .with_context(|| format!("download {url}"))?;
    if !response.status().is_success() {
        bail!("download {url} failed with {}", response.status());
    }
    let bytes = response.bytes().context("read download body")?;
    std::fs::write(path, &bytes).with_context(|| format!("write {}", path.display()))
}

fn verify_sha256(binary_path: &Path, checksum_path: &Path, asset_name: &str) -> Result<()> {
    let checksums = std::fs::read_to_string(checksum_path)
        .with_context(|| format!("read {}", checksum_path.display()))?;
    let expected = checksums
        .lines()
        .find_map(|line| {
            let mut parts = line.split_whitespace();
            let hash = parts.next()?;
            let name = parts.next()?.trim_start_matches('*');
            (name == asset_name).then_some(hash.to_string())
        })
        .ok_or_else(|| anyhow!("checksum for {asset_name} not found in SHA256SUMS"))?;
    let bytes =
        std::fs::read(binary_path).with_context(|| format!("read {}", binary_path.display()))?;
    let actual = format!("{:x}", Sha256::digest(bytes));
    if actual != expected.to_ascii_lowercase() {
        bail!("checksum mismatch for {asset_name}: expected {expected}, got {actual}");
    }
    Ok(())
}

fn wait_for_gateway_ready(gateway_url: &str) -> Result<()> {
    let base = gateway_url.trim_end_matches('/');
    let probe_targets = [
        format!("{base}/ready"),
        format!("{base}/api/ready"),
        format!("{base}/health"),
        format!("{base}/api/health"),
        format!("{base}/gateway/ready"),
        format!("{base}/gateway/health"),
        base.to_string(),
    ];
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_millis(1200))
        .build()
        .context("failed to build gateway readiness client")?;

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let mut last_err: Option<String> = None;

    while std::time::Instant::now() < deadline {
        for target in &probe_targets {
            match client.get(target).send() {
                Ok(response) => {
                    let status = response.status();
                    if status.is_success() {
                        if target.ends_with("/ready") {
                            return Ok(());
                        }

                        if target.ends_with("/health") || target == base {
                            let body_text = match response.text() {
                                Ok(text) => text,
                                Err(err) => {
                                    last_err = Some(format!(
                                        "{} returned {} but health payload could not be read: {err}",
                                        target, status
                                    ));
                                    continue;
                                }
                            };

                            let trimmed = body_text.trim();
                            let parsed = serde_json::from_str::<serde_json::Value>(trimmed);
                            if parsed
                                .as_ref()
                                .ok()
                                .and_then(|body| body.get("status"))
                                .and_then(|v| v.as_str())
                                == Some("ok")
                            {
                                return Ok(());
                            }

                            if trimmed.eq_ignore_ascii_case("ok") {
                                return Ok(());
                            }

                            match parsed {
                                Ok(body) => {
                                    last_err = Some(format!(
                                        "{} returned {} with non-ready health payload {}",
                                        target, status, body
                                    ));
                                }
                                Err(err) => {
                                    last_err = Some(format!(
                                        "{} returned {} but health payload could not be parsed: {err}; body={:?}",
                                        target, status, trimmed
                                    ));
                                }
                            }
                        } else {
                            return Ok(());
                        }
                    } else {
                        last_err = Some(format!("{} returned {}", target, status));
                    }
                }
                Err(err) => {
                    last_err = Some(format!("{} probe failed: {err}", target));
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }

    anyhow::bail!(
        "gateway did not become ready via {} ({})",
        probe_targets.join(", "),
        last_err.unwrap_or_else(|| "unknown error".to_string())
    )
}

fn print_update_wizard(install_script_url: &str, branch: &str) -> Result<()> {
    let exe = std::env::current_exe().context("failed to resolve current rune binary path")?;
    let repo_root =
        std::env::current_dir().context("failed to resolve current working directory")?;
    let repo_display = repo_root.display().to_string();
    let exe_display = exe.display().to_string();

    println!("Rune update wizard\n");
    println!("Fresh install:");
    println!("  curl -fsSL {install_script_url} | sh");
    println!();
    println!("Source checkout update ({repo_display}):");
    println!("  git pull --ff-only origin {branch}");
    println!("  cargo build --release --bin rune --bin rune-gateway");
    println!("  {exe_display} update status");
    println!();
    println!("Zero-config service install:");
    println!(
        r#"  rune setup --api-key "<YOUR_API_KEY>" --install-service --service-target systemd"#
    );
    println!(
        r#"  rune setup --api-key "<YOUR_API_KEY>" --install-service --service-target launchd"#
    );
    println!(
        "  # or: rune service install --target systemd --name rune-gateway --workdir ~/.rune --config ~/.rune/config.toml --enable --start"
    );
    println!("  # macOS direct service flow swaps --target launchd");
    println!();
    println!("Zero-config Docker Compose:");
    println!("  docker compose up --build -d");
    println!();
    println!("Then re-run first-run setup if needed:");
    println!(r#"  rune setup --api-key "<YOUR_API_KEY>""#);
    println!("  # or just: rune setup   # when Ollama is already running locally");
    println!();
    println!("Verify the install:");
    println!("  rune update wizard");
    println!("  rune --gateway-url http://127.0.0.1:8787 health");
    println!("  rune --gateway-url http://127.0.0.1:8787 doctor run");
    Ok(())
}

fn read_gateway_config_input(input: &str) -> Result<serde_json::Value> {
    let raw = if input == "-" {
        use std::io::Read as _;

        let mut stdin = String::new();
        std::io::stdin()
            .read_to_string(&mut stdin)
            .context("failed to read gateway config JSON from stdin")?;
        stdin
    } else {
        std::fs::read_to_string(input)
            .with_context(|| format!("failed to read gateway config JSON from {input}"))?
    };

    serde_json::from_str(&raw).with_context(|| {
        if input == "-" {
            "failed to parse gateway config JSON from stdin".to_string()
        } else {
            format!("failed to parse gateway config JSON from {input}")
        }
    })
}

fn parse_reminder_duration(input: &str) -> Result<DateTime<Utc>> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        anyhow::bail!("reminder duration cannot be empty");
    }

    let split_at = trimmed.find(|c: char| !c.is_ascii_digit()).ok_or_else(|| {
        anyhow::anyhow!("invalid reminder duration `{trimmed}`; expected forms like 30m, 2h, 1d")
    })?;
    let (amount_raw, unit_raw) = trimmed.split_at(split_at);
    let amount: i64 = amount_raw
        .parse()
        .with_context(|| format!("invalid reminder duration amount `{amount_raw}`"))?;
    if amount <= 0 {
        anyhow::bail!("reminder duration must be positive");
    }

    let delta = match unit_raw.trim().to_ascii_lowercase().as_str() {
        "m" | "min" | "mins" | "minute" | "minutes" => chrono::Duration::minutes(amount),
        "h" | "hr" | "hrs" | "hour" | "hours" => chrono::Duration::hours(amount),
        "d" | "day" | "days" => chrono::Duration::days(amount),
        other => {
            anyhow::bail!(
                "invalid reminder duration unit `{other}`; expected minutes (m), hours (h), or days (d)"
            )
        }
    };

    Ok(Utc::now() + delta)
}

fn set_default_model(model_ref: &str) -> Result<ModelSetResponse> {
    let config_path = discover_local_config_path();
    let config = rune_config::AppConfig::load(Some(&config_path)).with_context(|| {
        format!(
            "failed to load config from {} before updating default model",
            config_path.display()
        )
    })?;

    let resolved = config.models.resolve_model(model_ref).with_context(|| {
        format!("model `{model_ref}` is not resolvable from configured inventory")
    })?;
    let canonical = resolved.canonical_model_id();
    let inventory = config.models.model_ids();
    if !inventory.is_empty() && !inventory.iter().any(|entry| entry == &canonical) {
        anyhow::bail!("model `{model_ref}` is not present in configured inventory");
    }
    let previous = config.models.default_model.clone();

    let original = std::fs::read_to_string(&config_path)
        .with_context(|| format!("failed to read {}", config_path.display()))?;

    let mut lines = original
        .lines()
        .map(std::string::ToString::to_string)
        .collect::<Vec<_>>();

    let mut in_models = false;
    let mut replaced = false;
    let mut insert_at = None;

    for (idx, line) in lines.iter_mut().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            let section = trimmed.trim_matches(&['[', ']'][..]);
            if section == "models" {
                in_models = true;
                insert_at = Some(idx + 1);
                continue;
            }
            if in_models {
                insert_at = Some(idx);
                break;
            }
        }

        if in_models && trimmed.starts_with("default_model") {
            *line = format!("default_model = \"{canonical}\"");
            replaced = true;
            break;
        }
    }

    if !replaced {
        if let Some(idx) = insert_at {
            lines.insert(idx, format!("default_model = \"{canonical}\""));
        } else {
            if !lines.is_empty() && !lines.last().is_some_and(|line| line.is_empty()) {
                lines.push(String::new());
            }
            lines.push("[models]".to_string());
            lines.push(format!("default_model = \"{canonical}\""));
        }
    }

    let updated = format!("{}\n", lines.join("\n"));
    std::fs::write(&config_path, updated)
        .with_context(|| format!("failed to write {}", config_path.display()))?;

    Ok(ModelSetResponse {
        changed: previous.as_deref() != Some(canonical.as_str()),
        config_path: config_path.display().to_string(),
        previous_model: previous,
        default_model: canonical,
        note: "Local config updated; restart gateway to apply new default sessions.".to_string(),
    })
}

fn set_default_image_model(model_ref: &str) -> Result<ModelSetImageResponse> {
    let config_path = discover_local_config_path();
    let config = rune_config::AppConfig::load(Some(&config_path)).with_context(|| {
        format!(
            "failed to load config from {} before updating default image model",
            config_path.display()
        )
    })?;

    let resolved = config.models.resolve_model(model_ref).with_context(|| {
        format!("model `{model_ref}` is not resolvable from configured inventory")
    })?;
    let canonical = resolved.canonical_model_id();
    let inventory = config.models.model_ids();
    if !inventory.is_empty() && !inventory.iter().any(|entry| entry == &canonical) {
        anyhow::bail!("model `{model_ref}` is not present in configured inventory");
    }
    let previous = config.models.default_image_model.clone();

    let original = std::fs::read_to_string(&config_path)
        .with_context(|| format!("failed to read {}", config_path.display()))?;

    let mut lines = original
        .lines()
        .map(std::string::ToString::to_string)
        .collect::<Vec<_>>();

    let mut in_models = false;
    let mut replaced = false;
    let mut insert_at = None;

    for (idx, line) in lines.iter_mut().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            let section = trimmed.trim_matches(&['[', ']'][..]);
            if section == "models" {
                in_models = true;
                insert_at = Some(idx + 1);
                continue;
            }
            if in_models {
                insert_at = Some(idx);
                break;
            }
        }

        if in_models && trimmed.starts_with("default_image_model") {
            *line = format!("default_image_model = \"{canonical}\"");
            replaced = true;
            break;
        }
    }

    if !replaced {
        if let Some(idx) = insert_at {
            lines.insert(idx, format!("default_image_model = \"{canonical}\""));
        } else {
            if !lines.is_empty() && !lines.last().is_some_and(|line| line.is_empty()) {
                lines.push(String::new());
            }
            lines.push("[models]".to_string());
            lines.push(format!("default_image_model = \"{canonical}\""));
        }
    }

    let updated = format!("{}\n", lines.join("\n"));
    std::fs::write(&config_path, updated)
        .with_context(|| format!("failed to write {}", config_path.display()))?;

    Ok(ModelSetImageResponse {
        changed: previous.as_deref() != Some(canonical.as_str()),
        config_path: config_path.display().to_string(),
        previous_image_model: previous,
        default_image_model: canonical,
        note: "Local config updated; restart gateway to apply new default image model.".to_string(),
    })
}

fn model_fallback_details() -> ModelFallbacksResponse {
    let config = load_config();
    let text_chains = config
        .models
        .fallbacks
        .iter()
        .map(|fb| ModelFallbackChainDetail {
            name: fb.name.clone(),
            kind: "text".to_string(),
            chain: fb.chain.clone(),
        })
        .collect();
    let image_chains = config
        .models
        .image_fallbacks
        .iter()
        .map(|fb| ModelFallbackChainDetail {
            name: fb.name.clone(),
            kind: "image".to_string(),
            chain: fb.chain.clone(),
        })
        .collect();
    ModelFallbacksResponse {
        text_chains,
        image_chains,
    }
}

fn image_model_fallback_details() -> ModelFallbacksResponse {
    let config = load_config();
    let image_chains = config
        .models
        .image_fallbacks
        .iter()
        .map(|fb| ModelFallbackChainDetail {
            name: fb.name.clone(),
            kind: "image".to_string(),
            chain: fb.chain.clone(),
        })
        .collect();
    ModelFallbacksResponse {
        text_chains: vec![],
        image_chains,
    }
}

fn channel_details() -> Vec<ChannelDetail> {
    let config = load_config();
    let telegram_configured = config
        .channels
        .telegram_token
        .as_deref()
        .is_some_and(|token| !token.trim().is_empty());
    let telegram_enabled = config
        .channels
        .enabled
        .iter()
        .any(|name| name == "telegram");

    vec![ChannelDetail {
        name: "telegram".to_string(),
        enabled: telegram_enabled,
        configured: telegram_configured,
        status: if telegram_enabled && telegram_configured {
            "ready".to_string()
        } else if telegram_configured {
            "configured".to_string()
        } else {
            "disabled".to_string()
        },
        capabilities: vec![
            "receive.message".to_string(),
            "receive.edit".to_string(),
            "send.message".to_string(),
            "send.reply".to_string(),
            "edit.message".to_string(),
            "delete.message".to_string(),
        ],
        notes: if telegram_configured {
            None
        } else {
            Some("Set channels.telegram_token and enable telegram in channels.enabled".to_string())
        },
    }]
}

fn resolve_channel(target: &str, channels: &[ChannelDetail]) -> ChannelResolveResponse {
    let normalized = target.trim().to_ascii_lowercase();
    let aliases = match normalized.as_str() {
        "tg" | "telegram-bot" | "telegram_bot" => vec!["telegram"],
        other => vec![other],
    };

    let channel = channels
        .iter()
        .find(|channel| {
            aliases
                .iter()
                .any(|alias| channel.name.eq_ignore_ascii_case(alias))
        })
        .cloned();

    ChannelResolveResponse {
        target: target.to_string(),
        matched: channel.is_some(),
        channel,
        note: if channels.is_empty() {
            Some(
                "No channels are currently described by the local config/runtime inventory."
                    .to_string(),
            )
        } else if normalized != "telegram" && aliases == vec![normalized.as_str()] {
            Some(format!(
                "Known channels: {}",
                channels
                    .iter()
                    .map(|channel| channel.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        } else {
            None
        },
    }
}

fn heartbeat_presence() -> HeartbeatPresenceResponse {
    let config = load_config();
    let workspace_root = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let path = workspace_root.join("HEARTBEAT.md");
    match std::fs::metadata(&path) {
        Ok(metadata) => {
            let modified_at = metadata
                .modified()
                .ok()
                .and_then(|ts| ts.duration_since(SystemTime::UNIX_EPOCH).ok())
                .and_then(|duration| {
                    chrono::DateTime::<Utc>::from_timestamp(duration.as_secs() as i64, 0)
                })
                .map(|ts| ts.to_rfc3339());
            HeartbeatPresenceResponse {
                workspace_root: workspace_root.display().to_string(),
                path: path.display().to_string(),
                present: true,
                modified_at,
                size_bytes: Some(metadata.len()),
                note: Some(format!(
                    "Scheduled sessions load HEARTBEAT.md; runtime memory dir is {}.",
                    config.paths.memory_dir.display()
                )),
            }
        }
        Err(_) => HeartbeatPresenceResponse {
            workspace_root: workspace_root.display().to_string(),
            path: path.display().to_string(),
            present: false,
            modified_at: None,
            size_bytes: None,
            note: Some("No HEARTBEAT.md present in the current workspace root.".to_string()),
        },
    }
}

fn channel_logs(filter: Option<&str>, limit: usize) -> ChannelLogsResponse {
    let config = load_config();
    let logs_dir = config.paths.logs_dir;
    let normalized_filter = filter.map(|value| value.trim().to_ascii_lowercase());

    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&logs_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let file_name = match path.file_name().and_then(|name| name.to_str()) {
                Some(name) => name.to_string(),
                None => continue,
            };
            if let Some(filter_value) = &normalized_filter {
                if !file_name.to_ascii_lowercase().contains(filter_value) {
                    continue;
                }
            }
            let metadata = match entry.metadata() {
                Ok(metadata) => metadata,
                Err(_) => continue,
            };
            let modified_at = metadata
                .modified()
                .ok()
                .and_then(|ts| ts.duration_since(SystemTime::UNIX_EPOCH).ok())
                .and_then(|duration| {
                    chrono::DateTime::<Utc>::from_timestamp(duration.as_secs() as i64, 0)
                })
                .map(|ts| ts.to_rfc3339());
            files.push(ChannelLogFile {
                path: path.display().to_string(),
                modified_at,
                size_bytes: metadata.len(),
            });
        }
    }

    files.sort_by(|left, right| {
        right
            .modified_at
            .cmp(&left.modified_at)
            .then_with(|| left.path.cmp(&right.path))
    });
    files.truncate(limit);

    let note = if !logs_dir.exists() {
        Some(
            "Configured logs_dir does not exist yet; no local channel logs are available."
                .to_string(),
        )
    } else if files.is_empty() {
        Some("No matching log files found in the configured logs_dir.".to_string())
    } else {
        Some("This is a local filesystem view of channel-related logs, not a remote provider log API.".to_string())
    };

    ChannelLogsResponse {
        logs_dir: logs_dir.display().to_string(),
        filter: filter.map(str::to_string),
        files,
        note,
    }
}

fn provider_credential_source(provider: &rune_config::ModelProviderConfig) -> String {
    if provider
        .api_key
        .as_deref()
        .is_some_and(|key| !key.trim().is_empty())
    {
        "api_key".to_string()
    } else if let Some(env_var) = provider.api_key_env.as_deref() {
        format!("env:{env_var}")
    } else {
        "env:OPENAI_API_KEY".to_string()
    }
}

fn provider_credentials_ready(provider: &rune_config::ModelProviderConfig) -> bool {
    provider
        .api_key
        .as_deref()
        .is_some_and(|key| !key.trim().is_empty())
        || provider
            .api_key_env
            .as_deref()
            .and_then(|env_var| std::env::var(env_var).ok())
            .is_some_and(|value| !value.trim().is_empty())
        || (provider.api_key_env.is_none()
            && std::env::var("OPENAI_API_KEY")
                .ok()
                .is_some_and(|value| !value.trim().is_empty()))
}

fn provider_notes(provider: &rune_config::ModelProviderConfig) -> Option<String> {
    match provider.kind.as_str() {
        "azure-openai" | "azure_openai" | "azure"
            if provider.deployment_name.is_none() || provider.api_version.is_none() =>
        {
            Some("Azure OpenAI requires deployment_name and api_version for parity.".to_string())
        }
        "azure-foundry" if !provider.base_url.contains("services.ai.azure.com") => {
            Some("Azure Foundry is expected to use an Azure AI Foundry base URL.".to_string())
        }
        _ => None,
    }
}

fn model_auth_details() -> ModelAuthResponse {
    let config = load_config();
    let providers = config
        .models
        .providers
        .iter()
        .map(|provider| {
            let auth_order = config
                .models
                .auth_orders
                .iter()
                .find(|entry| entry.provider == provider.name)
                .map(|entry| entry.order.clone())
                .unwrap_or_default();
            let mut notes = Vec::new();
            if provider.api_key.as_deref().is_some_and(|key| !key.trim().is_empty()) {
                notes.push(
                    "Direct api_key is configured in local config. Prefer api_key_env for cleaner operator rotation when possible.".to_string(),
                );
            } else if let Some(env_var) = provider.api_key_env.as_deref() {
                notes.push(format!(
                    "Use `rune config set models.providers.<n>.api_key_env \"{env_var}\"` or set `{env_var}` in the runtime environment before launch."
                ));
            } else {
                notes.push(
                    "No provider-specific api_key_env configured; runtime will fall back to provider defaults such as OPENAI_API_KEY when supported.".to_string(),
                );
            }
            if let Some(note) = provider_notes(provider) {
                notes.push(note);
            }
            if auth_order.is_empty() {
                notes.push(
                    "No explicit auth_order configured for this provider; default provider resolution order applies.".to_string(),
                );
            }
            ModelAuthProviderDetail {
                provider: provider.name.clone(),
                provider_kind: provider.kind.clone(),
                credential_source: provider_credential_source(provider),
                credentials_ready: provider_credentials_ready(provider),
                api_key_configured: provider
                    .api_key
                    .as_deref()
                    .is_some_and(|key| !key.trim().is_empty()),
                api_key_env: provider.api_key_env.clone(),
                auth_order,
                notes,
            }
        })
        .collect();
    ModelAuthResponse { providers }
}

fn model_provider_details() -> ModelListResponse {
    let config = load_config();
    let default_model = config.models.default_model.clone().or_else(|| {
        config
            .agents
            .default_agent()
            .and_then(|agent| config.agents.effective_model(agent))
            .map(ToOwned::to_owned)
    });

    let providers = config
        .models
        .providers
        .iter()
        .map(|provider| ModelProviderDetail {
            name: provider.name.clone(),
            kind: provider.kind.clone(),
            base_url: provider.base_url.clone(),
            default_model: default_model.clone(),
            model_alias: provider.model_alias.clone(),
            deployment_name: provider.deployment_name.clone(),
            api_version: provider.api_version.clone(),
            credential_source: provider_credential_source(provider),
            credentials_ready: provider_credentials_ready(provider),
            notes: provider_notes(provider),
        })
        .collect();

    ModelListResponse {
        default_model,
        providers,
    }
}

fn model_alias_details() -> ModelAliasesResponse {
    let config = load_config();
    let aliases = config
        .models
        .providers
        .iter()
        .filter_map(|provider| {
            provider.model_alias.as_ref().map(|alias| ModelAliasDetail {
                alias: alias.clone(),
                provider: provider.name.clone(),
                target_model: provider.models.first().map(|model| model.id().to_string()),
                provider_kind: provider.kind.clone(),
                base_url: provider.base_url.clone(),
                deployment_name: provider.deployment_name.clone(),
                api_version: provider.api_version.clone(),
                credentials_ready: provider_credentials_ready(provider),
                note: provider_notes(provider),
            })
        })
        .collect();

    ModelAliasesResponse { aliases }
}

async fn init_workspace(
    path: &std::path::Path,
    template: Option<&str>,
    _non_interactive: bool,
) -> Result<()> {
    tokio::fs::create_dir_all(path)
        .await
        .with_context(|| format!("cannot create directory: {}", path.display()))?;
    tokio::fs::create_dir_all(path.join("memory")).await?;
    tokio::fs::create_dir_all(path.join("templates")).await?;

    let files: &[(&str, &str)] = &[
        (
            "AGENTS.md",
            "# AGENTS.md - Your Workspace\n\nAdd your agent configuration here.\n",
        ),
        (
            "SOUL.md",
            "# SOUL.md - Who You Are\n\nDefine your assistant's personality and style.\n",
        ),
        (
            "USER.md",
            "# USER.md - About Your Human\n\n- **Name:**\n- **Timezone:**\n- **Notes:**\n",
        ),
        (
            "TOOLS.md",
            "# TOOLS.md - Local Notes\n\nAdd environment-specific tool notes here.\n",
        ),
        (
            "MEMORY.md",
            "# MEMORY.md\n\nLong-term memory — curated and updated over time.\n",
        ),
    ];

    let mut created = 0;
    for (name, content) in files {
        let file_path = path.join(name);
        if !file_path.exists() {
            tokio::fs::write(&file_path, content).await?;
            created += 1;
            println!("  ✓ Created {name}");
        } else {
            println!("  ○ {name} already exists, skipping");
        }
    }

    // Bootstrap from template if specified
    if let Some(slug) = template {
        let tpl = rune_core::builtin_template_by_slug(slug)
            .ok_or_else(|| anyhow::anyhow!(
                "unknown template \"{slug}\". Run `rune agents templates` to see available templates."
            ))?;
        let spells_toml: Vec<String> = tpl.spells.iter().map(|s| format!("\"{s}\"")).collect();
        let config_content = format!(
            "# Auto-generated from template: {}\n\n[agent]\nmode = \"{}\"\nspells = [{}]\n",
            tpl.name,
            tpl.mode,
            spells_toml.join(", ")
        );
        let config_path = path.join("config.toml");
        if !config_path.exists() {
            tokio::fs::write(&config_path, &config_content).await?;
            created += 1;
            println!("  ✓ Created config.toml (from template: {})", tpl.name);
        } else {
            println!("  ○ config.toml already exists, skipping template");
        }
    }

    // Discover workspace-local templates
    let templates_dir = path.join("templates");
    if templates_dir.is_dir() {
        let mut count = 0u32;
        let mut entries = tokio::fs::read_dir(&templates_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let p = entry.path();
            if p.extension().is_some_and(|e| e == "toml") {
                count += 1;
            }
        }
        if count > 0 {
            println!("  ℹ Found {count} workspace template(s) in templates/");
        }
    }

    let memory_readme = path.join("memory/README.md");
    if !memory_readme.exists() {
        tokio::fs::write(
            &memory_readme,
            include_str!("../templates/workspace/memory/README.md"),
        )
        .await?;
        created += 1;
        println!("  ✓ Created memory/README.md");
    } else {
        println!("  ○ memory/README.md already exists, skipping");
    }

    let config_example_path = path.join("config.example.toml");
    if !config_example_path.exists() {
        tokio::fs::write(
            &config_example_path,
            include_str!("../../../config.example.toml"),
        )
        .await?;
        created += 1;
        println!("  ✓ Created config.example.toml");
    } else {
        println!("  ○ config.example.toml already exists, skipping");
    }

    println!(
        "\nWorkspace initialized at {} ({created} files created)",
        path.display()
    );
    Ok(())
}

/// Check bypass acknowledgment and prompt for first-use confirmation when
/// `--yolo` or `--no-sandbox` is active.  Returns `Ok(())` when the operator
/// has acknowledged (or `--accept-risk` was passed), or `Err` if they decline.
fn confirm_bypass_if_needed(cli: &Cli) -> Result<()> {
    use rune_config::{BypassAcknowledgment, BypassPosture};
    use std::io::{BufRead, IsTerminal, Write};

    // Build a minimal config to detect the posture.  The env vars have
    // already been set by `apply_global_cli_environment`.
    let mut probe = rune_config::AppConfig::default();
    probe.apply_cli_overrides(cli.yolo, cli.no_sandbox);

    let posture = match BypassPosture::detect(&probe) {
        Some(p) => p,
        None => return Ok(()), // standard mode, nothing to confirm
    };

    let ack = BypassAcknowledgment::from_home()
        .unwrap_or_else(|| BypassAcknowledgment::new(std::path::Path::new("/tmp/.rune-config")));

    if ack.is_acknowledged() {
        // Already acknowledged — emit a brief reminder on stderr.
        eprintln!("{}", posture.acknowledged_reminder());
        return Ok(());
    }

    // First use: show the full warning.
    eprintln!("{}", posture.first_use_warning());

    if cli.accept_risk {
        // Non-interactive acknowledgment (CI/scripts).
        eprintln!("--accept-risk passed; acknowledging bypass risk automatically.");
        ack.record().ok(); // best-effort persistence
        return Ok(());
    }

    // Interactive confirmation — only if stdin is a TTY.
    if !std::io::stdin().is_terminal() {
        anyhow::bail!(
            "Bypass mode requires acknowledgment. \
             Pass --accept-risk in non-interactive contexts."
        );
    }

    eprint!("Type YES to acknowledge and continue: ");
    std::io::stderr().flush().ok();

    let mut input = String::new();
    std::io::stdin().lock().read_line(&mut input)?;

    if input.trim() == "YES" {
        ack.record().ok(); // best-effort persistence
        Ok(())
    } else {
        anyhow::bail!("Bypass not acknowledged — aborting.");
    }
}

/// Execute the parsed CLI command against the configured gateway and print output.
pub async fn run(cli: Cli) -> Result<()> {
    apply_global_cli_environment(&cli);

    // First-use bypass confirmation (issue #64).
    confirm_bypass_if_needed(&cli)?;

    let format = OutputFormat::from_json_flag(cli.json);
    let client = GatewayClient::new(&cli.gateway_url);

    // Capture bypass flags before the match moves fields out of `cli`.
    let bypass_yolo = cli.yolo;
    let bypass_no_sandbox = cli.no_sandbox;

    match cli.command {
        Command::Gateway { action } | Command::Daemon { action } => match action {
            GatewayAction::Status => {
                let result = client.gateway_status().await?;
                println!("{}", render(&result, format));
            }
            GatewayAction::Health => {
                let result = client.gateway_health().await?;
                println!("{}", render(&result, format));
            }
            GatewayAction::Config { action } => match action {
                GatewayConfigAction::Show => {
                    let result = client.gateway_config().await?;
                    println!("{}", render(&result, format));
                }
                GatewayConfigAction::Apply { input } => {
                    let config = read_gateway_config_input(&input)?;
                    let result = client.gateway_config_apply(config).await?;
                    println!("{}", render(&result, format));
                }
            },
            GatewayAction::Runtime { action } => match action {
                GatewayRuntimeAction::Heartbeat { action } => match action {
                    GatewayRuntimeHeartbeatAction::Enable => {
                        let result = client.heartbeat_enable().await?;
                        println!("{}", render(&result, format));
                    }
                    GatewayRuntimeHeartbeatAction::Disable => {
                        let result = client.heartbeat_disable().await?;
                        println!("{}", render(&result, format));
                    }
                    GatewayRuntimeHeartbeatAction::Status => {
                        let result = client.heartbeat_status().await?;
                        println!("{}", render(&result, format));
                    }
                },
            },
            GatewayAction::Probe => {
                let result = client.gateway_probe().await?;
                println!("{}", render(&result, format));
            }
            GatewayAction::Discover => {
                let result = client.gateway_discover().await?;
                println!("{}", render(&result, format));
            }
            GatewayAction::Logs(LogsArgs {
                level,
                source,
                limit,
                since,
            }) => {
                let result = client
                    .logs_query(level.as_deref(), source.as_deref(), limit, since.as_deref())
                    .await?;
                println!("{}", render(&result, format));
            }
            GatewayAction::Doctor { action } => {
                let result = match action.unwrap_or(DoctorAction::Run) {
                    DoctorAction::Run => client.doctor_run().await?,
                    DoctorAction::Results => client.doctor_results().await?,
                };
                println!("{}", render(&result, format));
            }
            GatewayAction::Call {
                method,
                path,
                body,
                token,
            } => {
                let result = client
                    .gateway_call(&method, &path, body.as_deref(), token.as_deref())
                    .await?;
                println!("{}", render(&result, format));
            }
            GatewayAction::UsageCost => {
                let result = client.gateway_usage_cost().await?;
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
            GatewayAction::Run => {
                run_gateway_foreground(bypass_yolo, bypass_no_sandbox)?;
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
        Command::Logs { action } => match action {
            LogsAction::Query(LogsArgs {
                level,
                source,
                limit,
                since,
            }) => {
                let result = client
                    .logs_query(level.as_deref(), source.as_deref(), limit, since.as_deref())
                    .await?;
                println!("{}", render(&result, format));
            }
            LogsAction::Tail {
                level,
                source,
                follow,
                lines,
            } => {
                let http = reqwest::Client::new();
                let result = logs::tail(
                    &cli.gateway_url,
                    &http,
                    level.as_deref(),
                    source.as_deref(),
                    follow,
                    lines,
                )
                .await?;
                println!("{}", render(&result, format));
            }
            LogsAction::Search {
                query,
                level,
                source,
                limit,
            } => {
                let http = reqwest::Client::new();
                let result = logs::search(
                    &cli.gateway_url,
                    &http,
                    &query,
                    level.as_deref(),
                    source.as_deref(),
                    limit,
                )
                .await?;
                println!("{}", render(&result, format));
            }
            LogsAction::Export {
                format: fmt,
                level,
                source,
                since,
                until,
                limit,
                output,
            } => {
                let http = reqwest::Client::new();
                let result = logs::export(
                    &cli.gateway_url,
                    &http,
                    &fmt,
                    level.as_deref(),
                    source.as_deref(),
                    since.as_deref(),
                    until.as_deref(),
                    limit,
                    output.as_deref(),
                )
                .await?;
                println!("{}", render(&result, format));
            }
        },
        Command::Doctor { action } => {
            let result = match action.unwrap_or(DoctorAction::Run) {
                DoctorAction::Run => client.doctor_run().await?,
                DoctorAction::Results => client.doctor_results().await?,
            };
            println!("{}", render(&result, format));
        }
        Command::Dashboard => {
            let gateway = client.status().await?;
            let health = client.health().await?;
            let cron = client.cron_status().await?;
            let sessions = client.sessions_list(None, None, None, None, 5).await?;
            let channels = channel_details();
            let models = model_provider_details();
            let memory = memory::status().await?;

            let dashboard = DashboardResponse {
                gateway,
                health,
                cron,
                sessions: DashboardSessionsSummary {
                    total: sessions.sessions.len(),
                    sample: sessions.sessions,
                },
                models: DashboardModelsSummary {
                    total: models.providers.len(),
                    credentials_ready: models
                        .providers
                        .iter()
                        .filter(|provider| provider.credentials_ready)
                        .count(),
                    default_model: models.default_model,
                },
                channels: DashboardChannelsSummary {
                    total: channels.len(),
                    enabled: channels.iter().filter(|channel| channel.enabled).count(),
                    configured: channels.iter().filter(|channel| channel.configured).count(),
                    ready: channels
                        .iter()
                        .filter(|channel| channel.status == "ready")
                        .count(),
                },
                memory,
            };
            println!("{}", render(&dashboard, format));
        }
        Command::Init {
            path,
            api_key,
            provider,
            model,
            telegram_token,
            webchat,
            start,
            no_start,
            open,
            no_open,
            print_url,
            non_interactive,
            install_service,
            service_target,
            service_name,
            service_enable,
            no_service_enable,
            service_start,
            no_service_start,
        } => {
            run_init_wizard(InitWizardOptions {
                path: &path,
                api_key,
                provider,
                model,
                telegram_token,
                webchat,
                start: start && !no_start,
                open: open && !no_open,
                print_url,
                non_interactive,
                install_service,
                service_target,
                service_name: &service_name,
                service_enable: service_enable && !no_service_enable,
                service_start: service_start && !no_service_start,
                print_next_steps: true,
            })
            .await?;
        }
        Command::Skills { action } => match action {
            SkillsAction::List => {
                let result = client.skills_list().await?;
                println!("{}", render(&result, format));
            }
            SkillsAction::Info { name } => {
                let result = client.skills_info(&name).await?;
                println!("{}", render(&result, format));
            }
            SkillsAction::Check => {
                let result = client.skills_check().await?;
                println!("{}", render(&result, format));
            }
            SkillsAction::Enable { name } => {
                let result = client.skills_enable(&name).await?;
                println!("{}", render(&result, format));
            }
            SkillsAction::Disable { name } => {
                let result = client.skills_disable(&name).await?;
                println!("{}", render(&result, format));
            }
        },
        Command::Spells { action } => match action {
            SpellsAction::Search { query } => {
                let result: SpellSearchResponse = client.spells_search(&query).await?;
                println!("{}", render(&result, format));
            }
            SpellsAction::List => {
                let result = client.skills_list().await?;
                println!("{}", render(&result, format));
            }
            SpellsAction::Info { name } => {
                let result = client.skills_info(&name).await?;
                println!("{}", render(&result, format));
            }
            SpellsAction::Check => {
                let result = client.skills_check().await?;
                println!("{}", render(&result, format));
            }
            SpellsAction::Enable { name } => {
                let result = client.skills_enable(&name).await?;
                println!("{}", render(&result, format));
            }
            SpellsAction::Disable { name } => {
                let result = client.skills_disable(&name).await?;
                println!("{}", render(&result, format));
            }
        },
        Command::Completion { action } => match action {
            CompletionAction::Generate { shell } => {
                print_completion(shell)?;
            }
        },
        Command::Approvals { action } => match action {
            ApprovalsAction::List => {
                let result = client.approvals_list().await?;
                println!("{}", render(&result, format));
            }
            ApprovalsAction::Decide { id, decision, by } => {
                let result = client
                    .approvals_decide(&id, &decision, by.as_deref())
                    .await?;
                println!("{}", render(&result, format));
            }
            ApprovalsAction::Policies => {
                let result = client.approvals_policies_list().await?;
                println!("{}", render(&result, format));
            }
            ApprovalsAction::Get { tool } => {
                let result = client.approvals_get(&tool).await?;
                println!("{}", render(&result, format));
            }
            ApprovalsAction::Set { tool, decision } => {
                let result = client.approvals_set(&tool, &decision).await?;
                println!("{}", render(&result, format));
            }
            ApprovalsAction::Clear { tool } => {
                let result = client.approvals_clear(&tool).await?;
                println!("{}", render(&result, format));
            }
        },
        Command::Ms365 { action } => match action {
            Ms365Action::Auth { action } => match action {
                Ms365AuthAction::Status => {
                    let result = client.ms365_auth_status().await?;
                    println!("{}", render(&result, format));
                }
            },
            Ms365Action::Mail { action } => match action {
                Ms365MailAction::Unread { limit, folder } => {
                    let result = client.ms365_mail_unread(limit, &folder).await?;
                    println!("{}", render(&result, format));
                }
                Ms365MailAction::Read { id } => {
                    let result = client.ms365_mail_read(&id).await?;
                    println!("{}", render(&result, format));
                }
                Ms365MailAction::Folders => {
                    let result = client.ms365_mail_folders().await?;
                    println!("{}", render(&result, format));
                }
                Ms365MailAction::Send {
                    to,
                    subject,
                    body,
                    cc,
                } => {
                    let to: Vec<String> = to
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    let cc: Vec<String> = cc
                        .unwrap_or_default()
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    let result = client.ms365_mail_send(&to, &subject, &body, &cc).await?;
                    println!("{}", render(&result, format));
                }
                Ms365MailAction::Reply {
                    id,
                    body,
                    reply_all,
                } => {
                    let result = client.ms365_mail_reply(&id, &body, reply_all).await?;
                    println!("{}", render(&result, format));
                }
                Ms365MailAction::Forward { id, to, comment } => {
                    let to: Vec<String> = to
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    let result = client
                        .ms365_mail_forward(&id, &to, comment.as_deref())
                        .await?;
                    println!("{}", render(&result, format));
                }
                Ms365MailAction::Attachments { id } => {
                    let result = client.ms365_mail_attachments(&id).await?;
                    println!("{}", render(&result, format));
                }
                Ms365MailAction::AttachmentRead { message_id, id } => {
                    let result = client.ms365_mail_attachment_read(&message_id, &id).await?;
                    println!("{}", render(&result, format));
                }
                Ms365MailAction::AttachmentDownload {
                    message_id,
                    id,
                    output,
                } => {
                    let (filename, bytes) = client
                        .ms365_mail_attachment_download(&message_id, &id)
                        .await?;
                    let dest = output.unwrap_or_else(|| filename.clone());
                    tokio::fs::write(&dest, &bytes)
                        .await
                        .with_context(|| format!("failed to write attachment to {dest}"))?;
                    let result = crate::output::Ms365MailAttachmentDownloadResponse {
                        attachment_id: id,
                        filename,
                        size: bytes.len() as u64,
                        saved_to: dest,
                    };
                    println!("{}", render(&result, format));
                }
            },
            Ms365Action::Calendar { action } => match action {
                Ms365CalendarAction::Upcoming { limit, hours } => {
                    let result = client.ms365_calendar_upcoming(limit, hours).await?;
                    println!("{}", render(&result, format));
                }
                Ms365CalendarAction::Read { id } => {
                    let result = client.ms365_calendar_read(&id).await?;
                    println!("{}", render(&result, format));
                }
                Ms365CalendarAction::Create {
                    subject,
                    start,
                    end,
                    attendees,
                    location,
                    body,
                } => {
                    let attendees: Vec<String> = attendees
                        .unwrap_or_default()
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    let result = client
                        .ms365_calendar_create(
                            &subject,
                            &start,
                            &end,
                            &attendees,
                            location.as_deref(),
                            body.as_deref(),
                        )
                        .await?;
                    println!("{}", render(&result, format));
                }
                Ms365CalendarAction::Update {
                    id,
                    subject,
                    start,
                    end,
                    attendees,
                    location,
                    body,
                } => {
                    let attendees = attendees.map(|value| {
                        value
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect::<Vec<String>>()
                    });
                    let result = client
                        .ms365_calendar_update(
                            &id,
                            subject.as_deref(),
                            start.as_deref(),
                            end.as_deref(),
                            attendees.as_deref(),
                            location.as_deref(),
                            body.as_deref(),
                        )
                        .await?;
                    println!("{}", render(&result, format));
                }
                Ms365CalendarAction::Delete { id } => {
                    let result = client.ms365_calendar_delete(&id).await?;
                    println!("{}", render(&result, format));
                }
                Ms365CalendarAction::Respond {
                    id,
                    response,
                    comment,
                } => {
                    let result = client
                        .ms365_calendar_respond(&id, &response, comment.as_deref())
                        .await?;
                    println!("{}", render(&result, format));
                }
            },
            Ms365Action::Files { action } => match action {
                Ms365FilesAction::List { path, limit } => {
                    let result = client.ms365_files_list(&path, limit).await?;
                    println!("{}", render(&result, format));
                }
                Ms365FilesAction::Read { id } => {
                    let result = client.ms365_files_read(&id).await?;
                    println!("{}", render(&result, format));
                }
                Ms365FilesAction::Search { query, limit } => {
                    let result = client.ms365_files_search(&query, limit).await?;
                    println!("{}", render(&result, format));
                }
                Ms365FilesAction::Download { id, output } => {
                    let (filename, bytes) = client.ms365_files_download(&id).await?;
                    let dest = output.unwrap_or_else(|| filename.clone());
                    tokio::fs::write(&dest, &bytes)
                        .await
                        .with_context(|| format!("failed to write OneDrive file to {dest}"))?;
                    let result = crate::output::Ms365FileDownloadResponse {
                        item_id: id,
                        filename,
                        size: bytes.len() as u64,
                        saved_to: dest,
                    };
                    println!("{}", render(&result, format));
                }
            },
            Ms365Action::Users { action } => match action {
                Ms365UsersAction::Me => {
                    let result = client.ms365_users_me().await?;
                    println!("{}", render(&result, format));
                }
                Ms365UsersAction::List { limit } => {
                    let result = client.ms365_users_list(limit).await?;
                    println!("{}", render(&result, format));
                }
                Ms365UsersAction::Read { id } => {
                    let result = client.ms365_users_read(&id).await?;
                    println!("{}", render(&result, format));
                }
            },
            Ms365Action::Planner { action } => match action {
                Ms365PlannerAction::Plans { limit } => {
                    let result = client.ms365_planner_plans(limit).await?;
                    println!("{}", render(&result, format));
                }
                Ms365PlannerAction::Tasks { plan_id, limit } => {
                    let result = client.ms365_planner_tasks(&plan_id, limit).await?;
                    println!("{}", render(&result, format));
                }
                Ms365PlannerAction::TaskRead { id } => {
                    let result = client.ms365_planner_task_read(&id).await?;
                    println!("{}", render(&result, format));
                }
                Ms365PlannerAction::Create {
                    plan_id,
                    title,
                    bucket_id,
                    due_date,
                    description,
                } => {
                    let result = client
                        .ms365_planner_task_create(
                            &plan_id,
                            &title,
                            bucket_id.as_deref(),
                            due_date.as_deref(),
                            description.as_deref(),
                        )
                        .await?;
                    println!("{}", render(&result, format));
                }
                Ms365PlannerAction::Update {
                    id,
                    title,
                    bucket_id,
                    due_date,
                    description,
                    percent_complete,
                } => {
                    let result = client
                        .ms365_planner_task_update(
                            &id,
                            title.as_deref(),
                            bucket_id.as_deref(),
                            due_date.as_deref(),
                            description.as_deref(),
                            percent_complete,
                        )
                        .await?;
                    println!("{}", render(&result, format));
                }
                Ms365PlannerAction::Complete { id } => {
                    let result = client.ms365_planner_task_complete(&id).await?;
                    println!("{}", render(&result, format));
                }
            },
            Ms365Action::Todo { action } => match action {
                Ms365TodoAction::Lists { limit } => {
                    let result = client.ms365_todo_lists(limit).await?;
                    println!("{}", render(&result, format));
                }
                Ms365TodoAction::Tasks { list_id, limit } => {
                    let result = client.ms365_todo_tasks(&list_id, limit).await?;
                    println!("{}", render(&result, format));
                }
                Ms365TodoAction::TaskRead { list_id, id } => {
                    let result = client.ms365_todo_task_read(&list_id, &id).await?;
                    println!("{}", render(&result, format));
                }
                Ms365TodoAction::Create {
                    list_id,
                    title,
                    due_date,
                    importance,
                    body,
                } => {
                    let result = client
                        .ms365_todo_task_create(
                            &list_id,
                            &title,
                            due_date.as_deref(),
                            importance.as_deref(),
                            body.as_deref(),
                        )
                        .await?;
                    println!("{}", render(&result, format));
                }
                Ms365TodoAction::Update {
                    list_id,
                    id,
                    title,
                    status,
                    importance,
                    due_date,
                    body,
                } => {
                    let result = client
                        .ms365_todo_task_update(
                            &list_id,
                            &id,
                            crate::client::Ms365TodoTaskUpdateInput {
                                title,
                                status,
                                importance,
                                due_date,
                                body,
                            },
                        )
                        .await?;
                    println!("{}", render(&result, format));
                }
                Ms365TodoAction::Complete { list_id, id } => {
                    let result = client.ms365_todo_task_complete(&list_id, &id).await?;
                    println!("{}", render(&result, format));
                }
            },
            Ms365Action::Teams { action } => match action {
                Ms365TeamsAction::List { limit } => {
                    let result = client.ms365_teams_list(limit).await?;
                    println!("{}", render(&result, format));
                }
                Ms365TeamsAction::Channels { team_id, limit } => {
                    let result = client.ms365_teams_channels(&team_id, limit).await?;
                    println!("{}", render(&result, format));
                }
                Ms365TeamsAction::ChannelRead { team_id, id } => {
                    let result = client.ms365_teams_channel_read(&team_id, &id).await?;
                    println!("{}", render(&result, format));
                }
                Ms365TeamsAction::Messages {
                    team_id,
                    channel_id,
                    limit,
                } => {
                    let result = client
                        .ms365_teams_messages(&team_id, &channel_id, limit)
                        .await?;
                    println!("{}", render(&result, format));
                }
            },
            Ms365Action::Sites { action } => match action {
                Ms365SitesAction::List { limit } => {
                    let result = client.ms365_sites_list(limit).await?;
                    println!("{}", render(&result, format));
                }
                Ms365SitesAction::Read { id } => {
                    let result = client.ms365_sites_read(&id).await?;
                    println!("{}", render(&result, format));
                }
                Ms365SitesAction::Lists { site_id, limit } => {
                    let result = client.ms365_sites_lists(&site_id, limit).await?;
                    println!("{}", render(&result, format));
                }
                Ms365SitesAction::ListItems {
                    site_id,
                    list_id,
                    limit,
                } => {
                    let result = client
                        .ms365_sites_list_items(&site_id, &list_id, limit)
                        .await?;
                    println!("{}", render(&result, format));
                }
            },
        },
        Command::Process { action } => match action {
            ProcessAction::List => {
                let result = client.process_list().await?;
                println!("{}", render(&result, format));
            }
            ProcessAction::Get { id } => {
                let result = client.process_get(&id).await?;
                println!("{}", render(&result, format));
            }
            ProcessAction::Log { id } => {
                let log = client.process_log(&id).await?;
                print!("{log}");
            }
            ProcessAction::Kill { id } => {
                let result = client.process_kill(&id).await?;
                println!("{}", render(&result, format));
            }
        },
        Command::Cron { action } => match action {
            CronAction::Status => {
                let result = client.cron_status().await?;
                println!("{}", render(&result, format));
            }
            CronAction::List { include_disabled } => {
                let result = client.cron_list(include_disabled).await?;
                println!("{}", render(&result, format));
            }
            CronAction::Add {
                name,
                text,
                at,
                session_target,
                delivery_mode,
                webhook_url,
            } => {
                let at = DateTime::parse_from_rfc3339(&at)
                    .with_context(|| format!("invalid --at timestamp: {at}"))?
                    .with_timezone(&Utc);
                let result = client
                    .cron_add_system_event(
                        name.as_deref(),
                        &text,
                        at,
                        &session_target,
                        delivery_mode.as_str(),
                        webhook_url.as_deref(),
                    )
                    .await?;
                println!("{}", render(&result, format));
            }
            CronAction::Show { id } => {
                let result = client.cron_get(&id).await?;
                println!("{}", render(&result, format));
            }
            CronAction::Edit {
                id,
                name,
                delivery_mode,
                webhook_url,
            } => {
                let result = client
                    .cron_update(
                        &id,
                        name.as_deref(),
                        delivery_mode.map(CronDeliveryMode::as_str),
                        webhook_url.as_deref(),
                    )
                    .await?;
                println!("{}", render(&result, format));
            }
            CronAction::Enable { id } => {
                let result = client.cron_enable(&id).await?;
                println!("{}", render(&result, format));
            }
            CronAction::Disable { id } => {
                let result = client.cron_disable(&id).await?;
                println!("{}", render(&result, format));
            }
            CronAction::Rm { id } => {
                let result = client.cron_remove(&id).await?;
                println!("{}", render(&result, format));
            }
            CronAction::Run { id } => {
                let result = client.cron_run(&id).await?;
                println!("{}", render(&result, format));
            }
            CronAction::Runs { id } => {
                let result = client.cron_runs(&id).await?;
                println!("{}", render(&result, format));
            }
            CronAction::Wake {
                text,
                mode,
                context_messages,
            } => {
                let result = client
                    .cron_wake(&text, mode.as_str(), context_messages)
                    .await?;
                println!("{}", render(&result, format));
            }
        },
        Command::Sessions { action } => match action {
            SessionsAction::List {
                active_minutes,
                channel,
                kind,
                parent,
                limit,
            } => {
                let result = client
                    .sessions_list(
                        active_minutes,
                        channel.as_deref(),
                        kind.as_deref(),
                        parent.as_deref(),
                        limit,
                    )
                    .await?;
                println!("{}", render(&result, format));
            }
            SessionsAction::Show { id } => {
                let result = client.sessions_show(&id).await?;
                println!("{}", render(&result, format));
            }
            SessionsAction::Status { id } => {
                let result = client.session_status(&id).await?;
                println!("{}", render(&result, format));
            }
            SessionsAction::Tree { id } => {
                let result = client.sessions_tree(&id).await?;
                println!("{}", render(&result, format));
            }
            SessionsAction::History {
                id,
                kind,
                turn,
                tail,
            } => {
                use crate::output::SessionHistoryResponse;

                let all = client.sessions_transcript(&id).await?;
                let total_entries = all.len();

                // Apply filters.
                let mut entries: Vec<_> = all
                    .into_iter()
                    .filter(|e| kind.as_ref().is_none_or(|k| e.kind.eq_ignore_ascii_case(k)))
                    .filter(|e| {
                        turn.as_ref().is_none_or(|t| {
                            e.turn_id.as_deref().is_some_and(|tid| tid == t.as_str())
                        })
                    })
                    .collect();

                // Apply --tail (show last N).
                if let Some(n) = tail {
                    let skip = entries.len().saturating_sub(n);
                    entries = entries.into_iter().skip(skip).collect();
                }

                let shown = entries.len();
                let resp = SessionHistoryResponse {
                    session_id: id,
                    total_entries,
                    shown_entries: shown,
                    entries,
                };
                println!("{}", render(&resp, format));
            }
            SessionsAction::Export { id } => {
                use crate::output::SessionExportBundle;

                let session = client.sessions_show(&id).await?;
                let transcript = client.sessions_transcript(&id).await?;
                let bundle = SessionExportBundle {
                    session,
                    transcript,
                };
                // Export always emits JSON for machine consumption,
                // but respects --json flag for human-readable summary.
                println!("{}", render(&bundle, format));
            }
            SessionsAction::Delete { id } => {
                let result = client.session_delete(&id).await?;
                println!("{}", render(&result, format));
            }
            SessionsAction::Cleanup {
                status,
                kind,
                older_than_minutes,
                dry_run,
                limit,
            } => {
                use crate::output::{SessionCleanupItem, SessionCleanupResponse};

                // List sessions matching the filters.
                let list = client
                    .sessions_list(older_than_minutes, None, kind.as_deref(), None, limit)
                    .await?;

                // Apply client-side status filter (gateway list endpoint doesn't filter by status).
                let candidates: Vec<_> = list
                    .sessions
                    .into_iter()
                    .filter(|s| {
                        status
                            .as_ref()
                            .is_none_or(|f| s.status.eq_ignore_ascii_case(f))
                    })
                    .collect();

                if dry_run {
                    let items: Vec<SessionCleanupItem> = candidates
                        .iter()
                        .map(|s| SessionCleanupItem {
                            id: s.id.clone(),
                            kind: s.kind.clone(),
                            status: s.status.clone(),
                            result: "would_delete".to_string(),
                        })
                        .collect();
                    let resp = SessionCleanupResponse {
                        deleted: 0,
                        failed: 0,
                        dry_run: true,
                        sessions: items,
                    };
                    println!("{}", render(&resp, format));
                } else {
                    let mut items = Vec::new();
                    let mut deleted = 0usize;
                    let mut failed = 0usize;
                    for s in &candidates {
                        match client.session_delete(&s.id).await {
                            Ok(r) if r.success => {
                                deleted += 1;
                                items.push(SessionCleanupItem {
                                    id: s.id.clone(),
                                    kind: s.kind.clone(),
                                    status: s.status.clone(),
                                    result: "deleted".to_string(),
                                });
                            }
                            _ => {
                                failed += 1;
                                items.push(SessionCleanupItem {
                                    id: s.id.clone(),
                                    kind: s.kind.clone(),
                                    status: s.status.clone(),
                                    result: "failed".to_string(),
                                });
                            }
                        }
                    }
                    let resp = SessionCleanupResponse {
                        deleted,
                        failed,
                        dry_run: false,
                        sessions: items,
                    };
                    println!("{}", render(&resp, format));
                }
            }
        },
        Command::Agents { action } => match action {
            AgentsAction::List {
                active_minutes,
                limit,
            } => {
                let result = client.agents_list(active_minutes, limit).await?;
                println!("{}", render(&result, format));
            }
            AgentsAction::Show { id } => {
                let result = client.agents_show(&id).await?;
                println!("{}", render(&result, format));
            }
            AgentsAction::Status { id } => {
                let result = client.session_status(&id).await?;
                println!("{}", render(&result, format));
            }
            AgentsAction::Tree { limit } => {
                let sessions = client
                    .sessions_list(None, None, Some("subagent"), None, limit)
                    .await?;
                let root = sessions
                    .sessions
                    .first()
                    .map(|s| s.id.clone())
                    .unwrap_or_default();
                let result = client.sessions_tree(&root).await?;
                println!("{}", render(&result, format));
            }
            AgentsAction::Start { template } => {
                let tpl = rune_core::builtin_template_by_slug(&template)
                    .ok_or_else(|| anyhow::anyhow!(
                        "unknown template slug \"{template}\". Run `rune agents templates` to see available templates."
                    ))?;
                let session = client.sessions_create("subagent").await?;
                let result = TemplateStartResponse {
                    session_id: session.id,
                    template_slug: tpl.slug.to_string(),
                    template_name: tpl.name.to_string(),
                    mode: tpl.mode.to_string(),
                    status: session.status,
                };
                println!("{}", render(&result, format));
            }
            AgentsAction::Templates { category } => {
                let all = rune_core::builtin_agent_templates();
                let templates: Vec<TemplateSummary> = all
                    .iter()
                    .filter(|t| match category.as_deref() {
                        Some(c) => t.category.as_str() == c,
                        None => true,
                    })
                    .map(|t| TemplateSummary {
                        slug: t.slug.to_string(),
                        name: t.name.to_string(),
                        description: t.description.to_string(),
                        category: t.category.as_str().to_string(),
                        mode: t.mode.to_string(),
                        spells: t.spells.iter().map(|s| (*s).to_string()).collect(),
                    })
                    .collect();
                let result = TemplateListResponse { templates };
                println!("{}", render(&result, format));
            }
            AgentsAction::Spawn {
                parent,
                mode,
                policy,
                task,
                provider,
            } => {
                let result = client
                    .agent_spawn(&parent, &mode, &policy, &task, provider.as_deref())
                    .await?;
                println!("{}", render(&result, format));
            }
            AgentsAction::Steer { id, message } => {
                let result = client.agent_steer(&id, &message).await?;
                println!("{}", render(&result, format));
            }
            AgentsAction::Kill { id, reason } => {
                let result = client.agent_kill(&id, reason.as_deref()).await?;
                println!("{}", render(&result, format));
            }
        },
        Command::Channels { action } => {
            let channels = channel_details();
            match action {
                ChannelsAction::List => {
                    let result = ChannelListResponse { channels };
                    println!("{}", render(&result, format));
                }
                ChannelsAction::Status => {
                    let ready = channels
                        .iter()
                        .filter(|channel| channel.status == "ready")
                        .count();
                    let result = ChannelStatusResponse {
                        total: channels.len(),
                        enabled: channels.iter().filter(|channel| channel.enabled).count(),
                        configured: channels.iter().filter(|channel| channel.configured).count(),
                        ready,
                        channels,
                    };
                    println!("{}", render(&result, format));
                }
                ChannelsAction::Capabilities => {
                    let result = ChannelCapabilitiesResponse { channels };
                    println!("{}", render(&result, format));
                }
                ChannelsAction::Resolve { target } => {
                    let result = resolve_channel(&target, &channels);
                    println!("{}", render(&result, format));
                }
                ChannelsAction::Logs { channel, limit } => {
                    let result = channel_logs(channel.as_deref(), limit);
                    println!("{}", render(&result, format));
                }
            }
        }
        Command::Models { action } => {
            let result = model_provider_details();
            match action {
                ModelsAction::List => {
                    println!("{}", render(&result, format));
                }
                ModelsAction::Status => {
                    let ready = result
                        .providers
                        .iter()
                        .filter(|provider| provider.credentials_ready)
                        .count();
                    let status = ModelStatusResponse {
                        default_model: result.default_model,
                        total: result.providers.len(),
                        credentials_ready: ready,
                        providers: result.providers,
                    };
                    println!("{}", render(&status, format));
                }
                ModelsAction::Aliases => {
                    let result = model_alias_details();
                    println!("{}", render(&result, format));
                }
                ModelsAction::Auth => {
                    let result = model_auth_details();
                    println!("{}", render(&result, format));
                }
                ModelsAction::Set { model } => {
                    let result = set_default_model(&model)?;
                    println!("{}", render(&result, format));
                }
                ModelsAction::SetImage { model } => {
                    let result = set_default_image_model(&model)?;
                    println!("{}", render(&result, format));
                }
                ModelsAction::Fallbacks => {
                    let result = model_fallback_details();
                    println!("{}", render(&result, format));
                }
                ModelsAction::ImageFallbacks => {
                    let result = image_model_fallback_details();
                    println!("{}", render(&result, format));
                }
                ModelsAction::Scan => {
                    let result: ModelScanResponse = client.models_scan().await?;
                    println!("{}", render(&result, format));
                }
            }
        }
        Command::Memory { action } => match action {
            MemoryAction::Status => {
                let result = memory::status().await?;
                println!("{}", render(&result, format));
            }
            MemoryAction::Search { query, max_results } => {
                let result = memory::search(&query, max_results).await?;
                println!("{}", render(&result, format));
            }
            MemoryAction::Get { path, from, lines } => {
                let result = memory::get(&path, from, lines).await?;
                println!("{}", render(&result, format));
            }
        },
        Command::System { action } => match action {
            SystemAction::Event { action } => match action {
                SystemEventAction::Inject {
                    text,
                    mode,
                    context_messages,
                } => {
                    let result = client
                        .cron_wake(&text, mode.as_str(), context_messages)
                        .await?;
                    println!("{}", render(&result, format));
                }
                SystemEventAction::Schedule {
                    text,
                    at,
                    name,
                    session_target,
                    delivery_mode,
                    webhook_url,
                } => {
                    let at = DateTime::parse_from_rfc3339(&at)
                        .with_context(|| format!("invalid --at timestamp: {at}"))?
                        .with_timezone(&Utc);
                    let result = client
                        .cron_add_system_event(
                            name.as_deref(),
                            &text,
                            at,
                            &session_target,
                            delivery_mode.as_str(),
                            webhook_url.as_deref(),
                        )
                        .await?;
                    println!("{}", render(&result, format));
                }
                SystemEventAction::List { include_disabled } => {
                    let result = client.system_event_list(include_disabled).await?;
                    println!("{}", render(&result, format));
                }
            },
            SystemAction::Heartbeat { action } => match action {
                SystemHeartbeatAction::Presence | SystemHeartbeatAction::Last => {
                    let result = heartbeat_presence();
                    println!("{}", render(&result, format));
                }
                SystemHeartbeatAction::Enable => {
                    let result = client.heartbeat_enable().await?;
                    println!("{}", render(&result, format));
                }
                SystemHeartbeatAction::Disable => {
                    let result = client.heartbeat_disable().await?;
                    println!("{}", render(&result, format));
                }
                SystemHeartbeatAction::Status => {
                    let result = client.heartbeat_status().await?;
                    println!("{}", render(&result, format));
                }
            },
        },
        Command::Message { action } => match action {
            MessageAction::Send {
                channel,
                text,
                session,
                thread,
            } => {
                let result = client
                    .message_send(&channel, &text, session.as_deref(), thread.as_deref())
                    .await?;
                println!("{}", render(&result, format));
            }
            MessageAction::Search {
                query,
                channel,
                session,
                limit,
            } => {
                let result = client
                    .message_search(&query, channel.as_deref(), session.as_deref(), limit)
                    .await?;
                println!("{}", render(&result, format));
            }
            MessageAction::Broadcast {
                text,
                channels,
                session,
            } => {
                let result = client
                    .message_broadcast(&text, &channels, session.as_deref())
                    .await?;
                println!("{}", render(&result, format));
            }
            MessageAction::React {
                message_id,
                emoji,
                remove,
                channel,
                session,
            } => {
                let result = client
                    .message_react(
                        &message_id,
                        &emoji,
                        remove,
                        channel.as_deref(),
                        session.as_deref(),
                    )
                    .await?;
                println!("{}", render(&result, format));
            }
            MessageAction::Edit {
                message_id,
                channel,
                text,
                session,
            } => {
                let result = client
                    .message_edit(&message_id, &channel, &text, session.as_deref())
                    .await?;
                println!("{}", render(&result, format));
            }
            MessageAction::Pin {
                message_id,
                channel,
                session,
            } => {
                let result = client
                    .message_pin(&message_id, false, channel.as_deref(), session.as_deref())
                    .await?;
                println!("{}", render(&result, format));
            }
            MessageAction::Unpin {
                message_id,
                channel,
                session,
            } => {
                let result = client
                    .message_pin(&message_id, true, channel.as_deref(), session.as_deref())
                    .await?;
                println!("{}", render(&result, format));
            }
            MessageAction::Read {
                message_id,
                channel,
                session,
            } => {
                let result = client
                    .message_read(&message_id, &channel, session.as_deref())
                    .await?;
                println!("{}", render(&result, format));
            }
            MessageAction::Delete {
                message_id,
                channel,
                session,
            } => {
                let result = client
                    .message_delete(&message_id, &channel, session.as_deref())
                    .await?;
                println!("{}", render(&result, format));
            }
            MessageAction::Thread { action } => match action {
                MessageThreadAction::List {
                    thread_id,
                    channel,
                    session,
                    limit,
                } => {
                    let result = client
                        .message_thread_list(
                            &thread_id,
                            channel.as_deref(),
                            session.as_deref(),
                            limit,
                        )
                        .await?;
                    println!("{}", render(&result, format));
                }
                MessageThreadAction::Reply {
                    thread_id,
                    channel,
                    text,
                    session,
                } => {
                    let result = client
                        .message_thread_reply(&thread_id, &channel, &text, session.as_deref())
                        .await?;
                    println!("{}", render(&result, format));
                }
            },
            MessageAction::Voice { action } => match action {
                MessageVoiceAction::Send {
                    text,
                    channel,
                    voice,
                    model,
                    session,
                    output,
                } => {
                    use output::MessageVoiceSendResponse;

                    let audio = client
                        .tts_synthesize(&text, voice.as_deref(), model.as_deref())
                        .await;
                    match audio {
                        Ok(bytes) => {
                            let bytes_len = bytes.len();
                            let output_path = if let Some(ref path) = output {
                                tokio::fs::write(path, &bytes)
                                    .await
                                    .with_context(|| format!("failed to write audio to {path}"))?;
                                Some(path.clone())
                            } else {
                                None
                            };
                            let send_result = client
                                .message_send(
                                    &channel,
                                    &format!("[voice] {text}"),
                                    session.as_deref(),
                                    None,
                                )
                                .await?;
                            let result = MessageVoiceSendResponse {
                                success: true,
                                channel: channel.clone(),
                                bytes_synthesized: bytes_len,
                                output_path,
                                channel_delivered: send_result.success,
                                message_id: send_result.message_id,
                                detail: if send_result.success {
                                    format!(
                                        "Synthesized {bytes_len} bytes and delivered to {channel}",
                                    )
                                } else {
                                    format!(
                                        "Synthesized {bytes_len} bytes but channel delivery failed: {}",
                                        send_result.detail,
                                    )
                                },
                            };
                            println!("{}", render(&result, format));
                        }
                        Err(e) => {
                            let result = MessageVoiceSendResponse {
                                success: false,
                                channel: channel.clone(),
                                bytes_synthesized: 0,
                                output_path: None,
                                channel_delivered: false,
                                message_id: None,
                                detail: format!("TTS synthesis failed: {e}"),
                            };
                            println!("{}", render(&result, format));
                        }
                    }
                }
                MessageVoiceAction::Status => {
                    let result = client.message_voice_status().await?;
                    println!("{}", render(&result, format));
                }
            },
            MessageAction::Ack {
                message_id,
                channel,
                session,
            } => {
                let result = client
                    .message_ack(&message_id, &channel, session.as_deref())
                    .await?;
                println!("{}", render(&result, format));
            }
            MessageAction::ListReactions {
                message_id,
                channel,
                session,
            } => {
                let result = client
                    .message_list_reactions(&message_id, channel.as_deref(), session.as_deref())
                    .await?;
                println!("{}", render(&result, format));
            }
            MessageAction::Tag { action } => match action {
                MessageTagAction::Add {
                    message_id,
                    tag,
                    channel,
                    session,
                } => {
                    let result = client
                        .message_tag_add(&message_id, &tag, channel.as_deref(), session.as_deref())
                        .await?;
                    println!("{}", render(&result, format));
                }
                MessageTagAction::Remove {
                    message_id,
                    tag,
                    channel,
                    session,
                } => {
                    let result = client
                        .message_tag_remove(
                            &message_id,
                            &tag,
                            channel.as_deref(),
                            session.as_deref(),
                        )
                        .await?;
                    println!("{}", render(&result, format));
                }
                MessageTagAction::List {
                    message_id,
                    channel,
                    session,
                } => {
                    let result = client
                        .message_tag_list(&message_id, channel.as_deref(), session.as_deref())
                        .await?;
                    println!("{}", render(&result, format));
                }
            },
        },
        Command::Reminders { action } => match action {
            RemindersAction::Add {
                message,
                duration,
                target,
            } => {
                let fire_at = parse_reminder_duration(&duration)?;
                let result = client.reminders_add(&message, fire_at, &target).await?;
                println!("{}", render(&result, format));
            }
            RemindersAction::List { include_delivered } => {
                let result = client.reminders_list(include_delivered).await?;
                println!("{}", render(&result, format));
            }
            RemindersAction::Cancel { id } => {
                let result = client.reminders_cancel(&id).await?;
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
            ConfigAction::File => {
                let result = config_file();
                println!("{}", render(&result, format));
            }
            ConfigAction::Get { key } => {
                let result = config_get(&key)?;
                println!("{}", render(&result, format));
            }
            ConfigAction::Set { key, value } => {
                let result = config_set(&key, &value)?;
                println!("{}", render(&result, format));
            }
            ConfigAction::Unset { key } => {
                let result = config_unset(&key)?;
                println!("{}", render(&result, format));
            }
            ConfigAction::Validate { file } => {
                let result = validate_config(file.as_deref());
                println!("{}", render(&result, format));
            }
            ConfigAction::Reload => {
                let result = client.config_reload().await?;
                println!("{}", render(&result, format));
            }
            ConfigAction::Diff => {
                let result = client.config_diff().await?;
                println!("{}", render(&result, format));
            }
            ConfigAction::Env => {
                let result = client.config_env().await?;
                println!("{}", render(&result, format));
            }
            ConfigAction::Export { output } => {
                let result = client.config_export().await?;
                if output == "-" {
                    println!("{}", render(&result, format));
                } else {
                    std::fs::write(&output, render(&result, format))?;
                    println!("Exported resolved config to {output}");
                }
            }
        },
        Command::Security { action } => match action {
            SecurityAction::Audit => {
                let result = client.security_audit().await?;
                println!("{}", render(&result, format));
            }
        },
        Command::Sandbox { action } => match action {
            SandboxAction::List => {
                let result = client.sandbox_list().await?;
                println!("{}", render(&result, format));
            }
            SandboxAction::Recreate => {
                let result = client.sandbox_recreate().await?;
                println!("{}", render(&result, format));
            }
            SandboxAction::Explain => {
                let result = client.sandbox_explain().await?;
                println!("{}", render(&result, format));
            }
        },
        Command::Secrets { action } => match action {
            SecretsAction::Reload => {
                let result = client.secrets_reload().await?;
                println!("{}", render(&result, format));
            }
            SecretsAction::Audit => {
                let result = client.secrets_audit().await?;
                println!("{}", render(&result, format));
            }
            SecretsAction::Configure => {
                let result = client.secrets_configure().await?;
                println!("{}", render(&result, format));
            }
            SecretsAction::Apply { input } => {
                let manifest = read_gateway_config_input(&input)?;
                let result = client.secrets_apply(manifest).await?;
                println!("{}", render(&result, format));
            }
        },
        Command::McpMemoryServer { rune_url } => {
            rune_mcp::memory_server::run_stdio_server(Some(rune_url))
                .await
                .map_err(|e| anyhow::anyhow!("MCP memory server error: {e}"))?;
        }
        Command::Configure => {
            run_init_wizard(InitWizardOptions {
                path: ".",
                api_key: None,
                provider: None,
                model: None,
                telegram_token: None,
                webchat: true,
                start: true,
                open: true,
                print_url: false,
                non_interactive: false,
                install_service: false,
                service_target: ServiceTarget::Systemd,
                service_name: "rune-gateway",
                service_enable: true,
                service_start: true,
                print_next_steps: true,
            })
            .await?;
        }
        Command::Wizard {
            path,
            api_key,
            provider,
            model,
            telegram_token,
            webchat,
            start,
            no_start,
            open,
            no_open,
            print_url,
            non_interactive,
            install_service,
            service_target,
            service_name,
            service_enable,
            no_service_enable,
            service_start,
            no_service_start,
        }
        | Command::Setup {
            path,
            api_key,
            provider,
            model,
            telegram_token,
            webchat,
            start,
            no_start,
            open,
            no_open,
            print_url,
            non_interactive,
            install_service,
            service_target,
            service_name,
            service_enable,
            no_service_enable,
            service_start,
            no_service_start,
        }
        | Command::Onboard {
            path,
            api_key,
            provider,
            model,
            telegram_token,
            webchat,
            start,
            no_start,
            open,
            no_open,
            print_url,
            non_interactive,
            install_service,
            service_target,
            service_name,
            service_enable,
            no_service_enable,
            service_start,
            no_service_start,
        } => {
            run_init_wizard(InitWizardOptions {
                path: &path,
                api_key,
                provider,
                model,
                telegram_token,
                webchat,
                start: start && !no_start,
                open: open && !no_open,
                print_url,
                non_interactive,
                install_service,
                service_target,
                service_name: &service_name,
                service_enable: service_enable && !no_service_enable,
                service_start: service_start && !no_service_start,
                print_next_steps: true,
            })
            .await?;
        }
        Command::Agent { action } => match action {
            AgentAction::Run {
                session,
                message,
                max_turns,
                wait,
            } => {
                let result = client
                    .agent_run(&session, &message, max_turns, wait)
                    .await?;
                println!("{}", render(&result, format));
            }
            AgentAction::Result { session, turn } => {
                let result = client.agent_result(&session, &turn).await?;
                println!("{}", render(&result, format));
            }
        },
        Command::Acp { action } => match action {
            AcpAction::Send { from, to, payload } => {
                let result = client.acp_send(&from, &to, &payload).await?;
                println!("{}", render(&result, format));
            }
            AcpAction::Inbox { session } => {
                let result = client.acp_inbox(&session).await?;
                println!("{}", render(&result, format));
            }
            AcpAction::Ack {
                message_id,
                session,
            } => {
                let result = client.acp_ack(&message_id, &session).await?;
                println!("{}", render(&result, format));
            }
        },
        Command::Plugins { action } => match action {
            PluginsAction::List => {
                let result = client.plugins_list().await?;
                println!("{}", render(&result, format));
            }
            PluginsAction::Info { name } => {
                let result = client.plugins_info(&name).await?;
                println!("{}", render(&result, format));
            }
            PluginsAction::Install { source } => {
                let result = client.plugins_mutate("install", &source).await?;
                println!("{}", render(&result, format));
            }
            PluginsAction::Uninstall { name } => {
                let result = client.plugins_mutate("uninstall", &name).await?;
                println!("{}", render(&result, format));
            }
            PluginsAction::Enable { name } => {
                let result = client.plugins_mutate("enable", &name).await?;
                println!("{}", render(&result, format));
            }
            PluginsAction::Disable { name } => {
                let result = client.plugins_mutate("disable", &name).await?;
                println!("{}", render(&result, format));
            }
            PluginsAction::Update { name } => {
                let result = client.plugins_mutate("update", &name).await?;
                println!("{}", render(&result, format));
            }
            PluginsAction::Doctor { name } => {
                let result = client.plugins_mutate("doctor", &name).await?;
                println!("{}", render(&result, format));
            }
        },
        Command::Hooks { action } => match action {
            HooksAction::List => {
                let result = client.hooks_list().await?;
                println!("{}", render(&result, format));
            }
            HooksAction::Info { name } => {
                let result = client.hooks_info(&name).await?;
                println!("{}", render(&result, format));
            }
            HooksAction::Check => {
                let result = client.hooks_check().await?;
                println!("{}", render(&result, format));
            }
            HooksAction::Enable { name } => {
                let result = client.hooks_mutate("enable", &name).await?;
                println!("{}", render(&result, format));
            }
            HooksAction::Disable { name } => {
                let result = client.hooks_mutate("disable", &name).await?;
                println!("{}", render(&result, format));
            }
            HooksAction::Install { source } => {
                let result = client.hooks_mutate("install", &source).await?;
                println!("{}", render(&result, format));
            }
            HooksAction::Update { name } => {
                let result = client.hooks_mutate("update", &name).await?;
                println!("{}", render(&result, format));
            }
        },

        Command::Backup { action } => match action {
            BackupAction::Create { label } => {
                let result = client.backup_create(label.as_deref()).await?;
                println!("{}", render(&result, format));
            }
            BackupAction::List => {
                let result = client.backup_list().await?;
                println!("{}", render(&result, format));
            }
            BackupAction::Restore { id, confirm } => {
                if !confirm {
                    anyhow::bail!("--confirm is required to restore a backup");
                }
                let result = client.backup_restore(&id).await?;
                println!("{}", render(&result, format));
            }
        },
        Command::Update { action } => match action {
            UpdateAction::Check => {
                let result = client.update_check().await?;
                println!("{}", render(&result, format));
            }
            UpdateAction::Apply {
                version,
                repo,
                binary_path,
            } => {
                let result = apply_self_update(&repo, version.as_deref(), binary_path.as_deref())?;
                println!("{}", render(&result, format));
            }
            UpdateAction::Status => {
                let result = client.update_status().await?;
                println!("{}", render(&result, format));
            }
            UpdateAction::InstallScript { install_script_url } => {
                println!("{install_script_url}");
            }
            UpdateAction::Wizard {
                install_script_url,
                branch,
            } => {
                print_update_wizard(&install_script_url, &branch)?;
            }
        },
        Command::Service { action } => match action {
            ServiceAction::Print {
                target,
                name,
                workdir,
                config,
                gateway_url,
                yolo,
                no_sandbox,
            } => {
                let result = print_service_definition(
                    target,
                    &name,
                    std::path::Path::new(&workdir),
                    config.as_deref(),
                    gateway_url.as_deref(),
                    yolo,
                    no_sandbox,
                )?;
                println!("{}", render(&result, format));
            }
            ServiceAction::Install {
                target,
                name,
                workdir,
                config,
                gateway_url,
                yolo,
                no_sandbox,
                output,
                enable,
                start,
                no_bootstrap,
            } => {
                let result = install_service_definition(ServiceInstallOptions {
                    target,
                    name,
                    workdir: std::path::PathBuf::from(workdir),
                    config,
                    gateway_url,
                    yolo,
                    no_sandbox,
                    output: output.map(std::path::PathBuf::from),
                    enable,
                    start,
                    auto_bootstrap: !no_bootstrap,
                })?;
                println!("{}", render(&result, format));
            }
        },
        Command::Reset { confirm } => {
            if !confirm {
                anyhow::bail!("--confirm is required to reset the workspace");
            }
            let result = client.reset().await?;
            println!("{}", render(&result, format));
        }
    }

    Ok(())
}

#[cfg(test)]
mod update_tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn verify_sha256_accepts_matching_entry() {
        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("rune-linux-x86_64");
        std::fs::write(&binary, b"hello world").unwrap();
        let checksum = dir.path().join("SHA256SUMS");
        std::fs::write(
            &checksum,
            format!("{:x}  rune-linux-x86_64\n", Sha256::digest(b"hello world")),
        )
        .unwrap();

        verify_sha256(&binary, &checksum, "rune-linux-x86_64").unwrap();
    }

    #[test]
    fn resolve_update_asset_requires_platform_binary_and_checksum() {
        let release = GitHubRelease {
            tag_name: "v1.2.3".to_string(),
            assets: vec![
                GitHubReleaseAsset {
                    name: format!("rune-{}-{}", std::env::consts::OS, std::env::consts::ARCH),
                    browser_download_url: "https://example.test/rune".to_string(),
                },
                GitHubReleaseAsset {
                    name: "SHA256SUMS".to_string(),
                    browser_download_url: "https://example.test/SHA256SUMS".to_string(),
                },
            ],
        };

        let asset = resolve_update_asset(&release, "rune").unwrap();
        assert_eq!(asset.version, "v1.2.3");
        assert_eq!(
            asset.asset_name,
            format!("rune-{}-{}", std::env::consts::OS, std::env::consts::ARCH)
        );
    }

    #[test]
    fn fetch_release_metadata_reads_latest_release() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let read = stream.read(&mut buf).unwrap();
            let request = String::from_utf8_lossy(&buf[..read]);
            assert!(request.starts_with("GET /repos/test/rune/releases/latest "));
            let body = r#"{"tag_name":"v9.9.9","assets":[{"name":"SHA256SUMS","browser_download_url":"https://example.test/SHA256SUMS"}]}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).unwrap();
        });

        let client = BlockingHttpClient::builder()
            .user_agent("rune-test")
            .build()
            .unwrap();
        let release = client
            .get(format!("http://{addr}/repos/test/rune/releases/latest"))
            .send()
            .unwrap()
            .json::<GitHubRelease>()
            .unwrap();
        assert_eq!(release.tag_name, "v9.9.9");
        handle.join().unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use tempfile::TempDir;

    #[test]
    fn cli_setup_alias_supports_negative_quickstart_flags() {
        let cli = Cli::try_parse_from([
            "rune",
            "setup",
            "--no-start",
            "--no-open",
            "--install-service",
            "--no-service-enable",
            "--no-service-start",
        ])
        .expect("setup flags should parse");

        match cli.command {
            Command::Setup {
                start,
                no_start,
                open,
                no_open,
                install_service,
                service_enable,
                no_service_enable,
                service_start,
                no_service_start,
                ..
            } => {
                assert!(start);
                assert!(no_start);
                assert!(open);
                assert!(no_open);
                assert!(install_service);
                assert!(service_enable);
                assert!(no_service_enable);
                assert!(service_start);
                assert!(no_service_start);
            }
            other => panic!("expected setup command, got {:?}", other),
        }
    }

    #[test]
    fn update_wizard_mentions_service_install_and_docker_quickstart() {
        let _guard = crate::test_env_lock()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let cwd = std::env::current_dir().unwrap();
        let stdout = std::process::Command::new("cargo")
            .args([
                "run",
                "--quiet",
                "-p",
                "rune-cli-app",
                "--bin",
                "rune",
                "--",
                "update",
                "wizard",
            ])
            .current_dir(cwd)
            .env_remove("RUNE_CONFIG")
            .output()
            .expect("update wizard should run");
        assert!(
            stdout.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&stdout.stderr)
        );
        let stdout = String::from_utf8_lossy(&stdout.stdout);
        assert!(stdout.contains("--install-service --service-target systemd"));
        assert!(stdout.contains("--service-target launchd"));
        assert!(stdout.contains("docker compose up --build -d"));
    }

    #[test]
    fn open_url_in_browser_fails_without_display_on_linux() {
        if !cfg!(target_os = "linux") {
            return;
        }

        let display = std::env::var_os("DISPLAY");
        let wayland = std::env::var_os("WAYLAND_DISPLAY");
        unsafe {
            std::env::remove_var("DISPLAY");
            std::env::remove_var("WAYLAND_DISPLAY");
        }

        let err = open_url_in_browser("http://127.0.0.1:8787/webchat").unwrap_err();
        assert!(
            err.to_string()
                .contains("no graphical browser session detected")
        );

        unsafe {
            match display {
                Some(value) => std::env::set_var("DISPLAY", value),
                None => std::env::remove_var("DISPLAY"),
            }
            match wayland {
                Some(value) => std::env::set_var("WAYLAND_DISPLAY", value),
                None => std::env::remove_var("WAYLAND_DISPLAY"),
            }
        }
    }

    #[test]
    fn wait_for_gateway_ready_accepts_api_ready_probe() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0_u8; 1024];
                let _ = std::io::Read::read(&mut stream, &mut buf);
                let response = concat!(
                    "HTTP/1.1 404 Not Found
",
                    "Content-Length: 0
",
                    "Connection: close

"
                );
                let _ = std::io::Write::write_all(&mut stream, response.as_bytes());
            }
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0_u8; 1024];
                let _ = std::io::Read::read(&mut stream, &mut buf);
                let body = r#"{"status":"ok","service":"rune-gateway","checks":[]}"#;
                let response = format!(
                    "HTTP/1.1 200 OK
Content-Type: application/json
Content-Length: {}
Connection: close

{}",
                    body.len(),
                    body
                );
                let _ = std::io::Write::write_all(&mut stream, response.as_bytes());
            }
        });

        wait_for_gateway_ready(&format!("http://{addr}")).unwrap();
        handle.join().unwrap();
    }

    #[test]
    fn wait_for_gateway_ready_accepts_healthy_server() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            stream
                .write_all(
                    b"HTTP/1.1 200 OK
content-length: 2
connection: close

ok",
                )
                .unwrap();
        });

        wait_for_gateway_ready(&format!("http://{}", addr)).unwrap();
        handle.join().unwrap();
    }

    #[test]
    fn wait_for_gateway_ready_errors_when_server_never_starts() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);

        let err = wait_for_gateway_ready(&format!("http://{}", addr)).unwrap_err();
        let message = err.to_string();
        eprintln!("ERR={message}");
        assert!(message.contains("gateway did not become ready"));
        assert!(message.contains("/health"));
        assert!(message.contains("/gateway/health"));
    }

    #[test]
    fn wait_for_gateway_ready_rejects_degraded_ready_status() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            for _ in 0..8 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buf = [0u8; 1024];
                let read = stream.read(&mut buf).unwrap();
                let request = String::from_utf8_lossy(&buf[..read]);
                let response = if request.starts_with("GET /ready ") {
                    br#"HTTP/1.1 503 Service Unavailable
content-type: application/json
content-length: 22
connection: close

{"status":"degraded"}"#
                        .as_slice()
                } else if request.starts_with("GET /health ") {
                    br#"HTTP/1.1 200 OK
content-type: application/json
content-length: 15
connection: close

{"status":"ok"}"#
                        .as_slice()
                } else {
                    b"HTTP/1.1 404 Not Found
content-length: 0
connection: close

"
                    .as_slice()
                };
                stream.write_all(response).unwrap();
            }
        });

        wait_for_gateway_ready(&format!("http://{}", addr)).unwrap();
        handle.join().unwrap();
    }

    #[test]
    fn wait_for_gateway_ready_rejects_unhealthy_health_payloads() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            for _ in 0..200 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buf = [0u8; 1024];
                let read = stream.read(&mut buf).unwrap();
                let request = String::from_utf8_lossy(&buf[..read]);
                let response = if request.starts_with("GET /ready ") {
                    b"HTTP/1.1 404 Not Found
content-length: 0
connection: close

"
                    .as_slice()
                } else if request.starts_with("GET /health ") {
                    br#"HTTP/1.1 200 OK
content-type: application/json
content-length: 22
connection: close

{"status":"degraded"}"#
                        .as_slice()
                } else if request.starts_with("GET /gateway/ready ") {
                    b"HTTP/1.1 404 Not Found
content-length: 0
connection: close

"
                    .as_slice()
                } else if request.starts_with("GET /gateway/health ") {
                    br#"HTTP/1.1 200 OK
content-type: application/json
content-length: 22
connection: close

{"status":"unhealthy"}"#
                        .as_slice()
                } else if request.starts_with("GET / ") {
                    br#"HTTP/1.1 200 OK
content-type: application/json
content-length: 22
connection: close

{"status":"degraded"}"#
                        .as_slice()
                } else {
                    b"HTTP/1.1 503 Service Unavailable
content-length: 0
connection: close

"
                    .as_slice()
                };
                stream.write_all(response).unwrap();
            }
        });

        let err = wait_for_gateway_ready(&format!("http://{}", addr)).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("gateway did not become ready"));
        assert!(
            message.contains("degraded")
                || message.contains("unhealthy")
                || message.contains("probe failed")
        );
        handle.join().unwrap();
    }

    #[test]
    fn wait_for_gateway_ready_accepts_plain_text_gateway_health() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buf = [0u8; 1024];
                let read = stream.read(&mut buf).unwrap();
                let request = String::from_utf8_lossy(&buf[..read]);
                let response = if request.starts_with("GET /ready ") {
                    b"HTTP/1.1 404 Not Found
content-length: 0
connection: close

"
                    .as_slice()
                } else if request.starts_with("GET /health ") {
                    b"HTTP/1.1 200 OK
content-type: text/plain
content-length: 2
connection: close

ok"
                    .as_slice()
                } else {
                    b"HTTP/1.1 500 Internal Server Error
content-length: 0
connection: close

"
                    .as_slice()
                };
                stream.write_all(response).unwrap();
            }
        });

        wait_for_gateway_ready(&format!("http://{}", addr)).unwrap();
        handle.join().unwrap();
    }

    #[test]
    fn wait_for_gateway_ready_falls_back_to_gateway_health_when_health_is_missing() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            for _ in 0..4 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buf = [0u8; 1024];
                let read = stream.read(&mut buf).unwrap();
                let request = String::from_utf8_lossy(&buf[..read]);
                let response = if request.starts_with("GET /ready ") {
                    b"HTTP/1.1 404 Not Found
content-length: 0
connection: close

"
                    .as_slice()
                } else if request.starts_with("GET /health ") {
                    b"HTTP/1.1 404 Not Found
content-length: 0
connection: close

"
                    .as_slice()
                } else if request.starts_with("GET /gateway/ready ") {
                    b"HTTP/1.1 404 Not Found
content-length: 0
connection: close

"
                    .as_slice()
                } else if request.starts_with("GET /gateway/health ") {
                    b"HTTP/1.1 200 OK
content-length: 2
connection: close

ok"
                    .as_slice()
                } else {
                    b"HTTP/1.1 500 Internal Server Error
content-length: 0
connection: close

"
                    .as_slice()
                };
                stream.write_all(response).unwrap();
            }
        });

        wait_for_gateway_ready(&format!("http://{}", addr)).unwrap();
        handle.join().unwrap();
    }

    #[test]
    fn write_wizard_config_uses_env_var_for_openai_api_key() {
        let tmp = TempDir::new().unwrap();
        let config_path =
            write_wizard_config(tmp.path(), "openai", "gpt-4o-mini", "", None, true).unwrap();

        let written = std::fs::read_to_string(config_path).unwrap();
        assert!(written.contains("api_key_env = \"OPENAI_API_KEY\""));
        assert!(!written.contains("api_key = \"\""));
    }

    #[test]
    fn write_wizard_config_keeps_inline_api_key_when_provided() {
        let tmp = TempDir::new().unwrap();
        let config_path =
            write_wizard_config(tmp.path(), "openai", "gpt-4o-mini", "test-key", None, true)
                .unwrap();

        let written = std::fs::read_to_string(config_path).unwrap();
        assert!(written.contains("api_key = \"test-key\""));
        assert!(!written.contains("api_key_env"));
    }

    #[test]
    fn setup_open_url_includes_api_key_query_for_browser_auth_flow() {
        let url = browser_launch_url(
            "http://127.0.0.1:8787",
            "/chat",
            "openai",
            "test key/with?chars",
            None,
        );

        assert_eq!(
            url,
            "http://127.0.0.1:8787/chat?api_key=test%20key%2Fwith%3Fchars&auth=test%20key%2Fwith%3Fchars"
        );
    }

    #[test]
    fn setup_open_url_includes_session_token_for_webchat_resume() {
        let url = browser_launch_url(
            "http://127.0.0.1:8787",
            "/webchat",
            "openai",
            "test-key",
            Some("browser token"),
        );

        assert_eq!(
            url,
            "http://127.0.0.1:8787/webchat?api_key=test-key&auth=test-key&session_token=browser%20token"
        );
    }

    #[test]
    fn setup_open_url_skips_auth_query_for_ollama() {
        let url = browser_launch_url(
            "http://127.0.0.1:8787",
            "/webchat",
            "ollama",
            "ignored",
            None,
        );
        assert_eq!(url, "http://127.0.0.1:8787/webchat");
    }

    #[test]
    fn open_url_in_browser_errors_cleanly_without_graphical_session() {
        let _guard = crate::test_env_lock()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        unsafe {
            std::env::remove_var("DISPLAY");
            std::env::remove_var("WAYLAND_DISPLAY");
        }

        let err = open_url_in_browser("http://127.0.0.1:8787/webchat").unwrap_err();
        let message = err.to_string();
        assert!(message.contains("no graphical browser session detected"));
        assert!(message.contains("--no-open"));
    }

    #[test]
    fn set_default_model_updates_existing_models_section() {
        let _guard = crate::test_env_lock()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = TempDir::new().unwrap();
        let config_path = tmp.path().join("config.toml");
        std::fs::write(
            &config_path,
            r#"[models]
default_model = "oc-01-openai/gpt-5.4"

[[models.providers]]
name = "oc-01-openai"
kind = "openai"
base_url = "https://example.test/openai/v1"
api_key = "test-key"
models = ["gpt-5.4", "gpt-5.4-pro"]
"#,
        )
        .unwrap();

        unsafe {
            std::env::set_var("RUNE_CONFIG", &config_path);
        }

        let response = set_default_model("oc-01-openai/gpt-5.4-pro").unwrap();
        assert!(response.changed);
        assert_eq!(
            response.previous_model.as_deref(),
            Some("oc-01-openai/gpt-5.4")
        );
        assert_eq!(response.default_model, "oc-01-openai/gpt-5.4-pro");

        let updated = std::fs::read_to_string(&config_path).unwrap();
        assert!(updated.contains("default_model = \"oc-01-openai/gpt-5.4-pro\""));

        unsafe {
            std::env::remove_var("RUNE_CONFIG");
        }
    }

    #[test]
    fn set_default_model_accepts_unambiguous_short_name() {
        let _guard = crate::test_env_lock()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = TempDir::new().unwrap();
        let config_path = tmp.path().join("config.toml");
        std::fs::write(
            &config_path,
            r#"[[models.providers]]
name = "hamza-eastus2"
kind = "openai"
base_url = "https://example.test/openai/v1"
api_key = "test-key"
models = ["grok-4-fast-reasoning"]
"#,
        )
        .unwrap();

        unsafe {
            std::env::set_var("RUNE_CONFIG", &config_path);
        }

        let response = set_default_model("grok-4-fast-reasoning").unwrap();
        assert_eq!(
            response.default_model,
            "hamza-eastus2/grok-4-fast-reasoning"
        );

        unsafe {
            std::env::remove_var("RUNE_CONFIG");
        }
    }

    #[test]
    fn set_default_image_model_updates_existing_models_section() {
        let _guard = crate::test_env_lock()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = TempDir::new().unwrap();
        let config_path = tmp.path().join("config.toml");
        std::fs::write(
            &config_path,
            r#"[models]
default_image_model = "oc-01-openai/dall-e-3"

[[models.providers]]
name = "oc-01-openai"
kind = "openai"
base_url = "https://example.test/openai/v1"
api_key = "test-key"
models = ["dall-e-3", "dall-e-4"]
"#,
        )
        .unwrap();

        unsafe {
            std::env::set_var("RUNE_CONFIG", &config_path);
        }

        let response = set_default_image_model("oc-01-openai/dall-e-4").unwrap();
        assert!(response.changed);
        assert_eq!(
            response.previous_image_model.as_deref(),
            Some("oc-01-openai/dall-e-3")
        );
        assert_eq!(response.default_image_model, "oc-01-openai/dall-e-4");

        let updated = std::fs::read_to_string(&config_path).unwrap();
        assert!(updated.contains("default_image_model = \"oc-01-openai/dall-e-4\""));

        unsafe {
            std::env::remove_var("RUNE_CONFIG");
        }
    }

    #[test]
    fn set_default_image_model_inserts_when_missing() {
        let _guard = crate::test_env_lock()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = TempDir::new().unwrap();
        let config_path = tmp.path().join("config.toml");
        std::fs::write(
            &config_path,
            r#"[models]

[[models.providers]]
name = "hamza-eastus2"
kind = "openai"
base_url = "https://example.test/openai/v1"
api_key = "test-key"
models = ["dall-e-3"]
"#,
        )
        .unwrap();

        unsafe {
            std::env::set_var("RUNE_CONFIG", &config_path);
        }

        let response = set_default_image_model("dall-e-3").unwrap();
        assert!(response.changed);
        assert_eq!(response.default_image_model, "hamza-eastus2/dall-e-3");
        assert!(response.previous_image_model.is_none());

        let updated = std::fs::read_to_string(&config_path).unwrap();
        assert!(updated.contains("default_image_model = \"hamza-eastus2/dall-e-3\""));

        unsafe {
            std::env::remove_var("RUNE_CONFIG");
        }
    }

    #[test]
    fn set_default_image_model_rejects_unknown_inventory_entry() {
        let _guard = crate::test_env_lock()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = TempDir::new().unwrap();
        let config_path = tmp.path().join("config.toml");
        std::fs::write(
            &config_path,
            r#"[[models.providers]]
name = "oc-01-openai"
kind = "openai"
base_url = "https://example.test/openai/v1"
api_key = "test-key"
models = ["dall-e-3"]
"#,
        )
        .unwrap();

        unsafe {
            std::env::set_var("RUNE_CONFIG", &config_path);
        }

        let err = set_default_image_model("not-a-real-model").unwrap_err();
        assert!(
            err.to_string()
                .contains("not present in configured inventory")
                || err.to_string().contains("not resolvable")
        );

        unsafe {
            std::env::remove_var("RUNE_CONFIG");
        }
    }

    #[test]
    fn parse_reminder_duration_minutes() {
        let fire_at = parse_reminder_duration("30m").unwrap();
        let delta = fire_at.signed_duration_since(Utc::now());
        assert!(delta.num_minutes() >= 29 && delta.num_minutes() <= 30);
    }

    #[test]
    fn parse_reminder_duration_hours() {
        let fire_at = parse_reminder_duration("2h").unwrap();
        let delta = fire_at.signed_duration_since(Utc::now());
        assert!(delta.num_minutes() >= 119 && delta.num_minutes() <= 120);
    }

    #[test]
    fn parse_reminder_duration_rejects_bad_unit() {
        let err = parse_reminder_duration("5w").unwrap_err();
        assert!(err.to_string().contains("invalid reminder duration unit"));
    }

    #[test]
    fn read_gateway_config_input_reads_json_file() {
        let tmp = TempDir::new().unwrap();
        let input_path = tmp.path().join("gateway-config.json");
        std::fs::write(
            &input_path,
            r#"{"gateway":{"host":"127.0.0.1","port":8787}}"#,
        )
        .unwrap();

        let config = read_gateway_config_input(input_path.to_str().unwrap()).unwrap();
        assert_eq!(config["gateway"]["host"], "127.0.0.1");
        assert_eq!(config["gateway"]["port"], 8787);
    }

    #[test]
    fn set_default_model_rejects_unknown_inventory_entry() {
        let _guard = crate::test_env_lock()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = TempDir::new().unwrap();
        let config_path = tmp.path().join("config.toml");
        std::fs::write(
            &config_path,
            r#"[[models.providers]]
name = "oc-01-openai"
kind = "openai"
base_url = "https://example.test/openai/v1"
api_key = "test-key"
models = ["gpt-5.4"]
"#,
        )
        .unwrap();

        unsafe {
            std::env::set_var("RUNE_CONFIG", &config_path);
        }

        let err = set_default_model("not-a-real-model").unwrap_err();
        assert!(
            err.to_string()
                .contains("not present in configured inventory")
                || err.to_string().contains("not resolvable")
        );

        unsafe {
            std::env::remove_var("RUNE_CONFIG");
        }
    }

    #[test]
    fn discover_local_config_path_uses_profile_when_present() {
        let _guard = crate::test_env_lock()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        unsafe {
            std::env::remove_var("RUNE_CONFIG");
            std::env::remove_var("RUNE_PROFILE");
            std::env::set_var("RUNE_PROFILE", "azure");
        }
        assert_eq!(
            discover_local_config_path(),
            std::path::PathBuf::from("config.azure.toml")
        );
        unsafe {
            std::env::remove_var("RUNE_PROFILE");
        }
    }

    /// Helper: generate a completion script into a buffer and return as a String.
    fn generate_completion_string(shell: cli::CompletionShell) -> String {
        let mut buf = Vec::new();
        let mut command = cli::Cli::command();
        clap_complete::generate(completion_shell(shell), &mut command, "rune", &mut buf);
        String::from_utf8(buf).expect("completion script should be valid UTF-8")
    }

    #[test]
    fn bash_completion_contains_subcommands() {
        let script = generate_completion_string(cli::CompletionShell::Bash);
        assert!(
            !script.is_empty(),
            "bash completion script must not be empty"
        );
        // The script should reference key top-level subcommands.
        for cmd in [
            "gateway",
            "status",
            "completion",
            "config",
            "doctor",
            "skills",
        ] {
            assert!(
                script.contains(cmd),
                "bash completion missing subcommand `{cmd}`",
            );
        }
    }

    #[test]
    fn zsh_completion_contains_subcommands() {
        let script = generate_completion_string(cli::CompletionShell::Zsh);
        assert!(
            !script.is_empty(),
            "zsh completion script must not be empty"
        );
        for cmd in [
            "gateway",
            "status",
            "completion",
            "config",
            "doctor",
            "skills",
        ] {
            assert!(
                script.contains(cmd),
                "zsh completion missing subcommand `{cmd}`",
            );
        }
    }

    #[test]
    fn fish_completion_contains_subcommands() {
        let script = generate_completion_string(cli::CompletionShell::Fish);
        assert!(
            !script.is_empty(),
            "fish completion script must not be empty"
        );
        for cmd in [
            "gateway",
            "status",
            "completion",
            "config",
            "doctor",
            "skills",
        ] {
            assert!(
                script.contains(cmd),
                "fish completion missing subcommand `{cmd}`",
            );
        }
    }

    #[test]
    fn completion_scripts_include_global_flags() {
        // Global flags like --json should appear in all shell completions.
        // Fish uses `-l json` instead of `--json`, so check for "json" broadly.
        for shell in [
            cli::CompletionShell::Bash,
            cli::CompletionShell::Zsh,
            cli::CompletionShell::Fish,
        ] {
            let script = generate_completion_string(shell);
            assert!(
                script.contains("json"),
                "{shell:?} completion missing json flag reference",
            );
        }
    }

    #[test]
    fn apply_global_cli_environment_sets_dev_profile_and_log_level() {
        let _guard = crate::test_env_lock()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        unsafe {
            std::env::remove_var("RUNE_PROFILE");
            std::env::remove_var("RUNE_LOG_LEVEL");
            std::env::remove_var("RUST_LOG");
            std::env::remove_var("NO_COLOR");
        }

        let cli = Cli::try_parse_from([
            "rune",
            "--dev",
            "--log-level",
            "trace",
            "--no-color",
            "status",
        ])
        .unwrap();
        apply_global_cli_environment(&cli);

        assert_eq!(std::env::var("RUNE_PROFILE").ok().as_deref(), Some("dev"));
        assert_eq!(
            std::env::var("RUNE_LOG_LEVEL").ok().as_deref(),
            Some("trace")
        );
        assert_eq!(std::env::var("RUST_LOG").ok().as_deref(), Some("trace"));
        assert_eq!(std::env::var("NO_COLOR").ok().as_deref(), Some("1"));

        unsafe {
            std::env::remove_var("RUNE_PROFILE");
            std::env::remove_var("RUNE_LOG_LEVEL");
            std::env::remove_var("RUST_LOG");
            std::env::remove_var("NO_COLOR");
        }
    }
}
