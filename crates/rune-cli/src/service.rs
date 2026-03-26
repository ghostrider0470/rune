use anyhow::{Context, Result};
use serde::Serialize;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::cli::ServiceTarget;

#[derive(Debug, Clone)]
pub struct ServiceInstallOptions {
    pub target: ServiceTarget,
    pub name: String,
    pub workdir: PathBuf,
    pub config: Option<String>,
    pub gateway_url: Option<String>,
    pub yolo: bool,
    pub no_sandbox: bool,
    pub output: Option<PathBuf>,
    pub enable: bool,
    pub start: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServiceDefinitionResponse {
    pub target: String,
    pub name: String,
    pub output_path: Option<String>,
    pub content: String,
    pub activation_commands: Vec<String>,
    pub notes: Vec<String>,
}

impl fmt::Display for ServiceDefinitionResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(path) = &self.output_path {
            writeln!(
                f,
                "✓ Wrote {} service definition for {} to {}",
                self.target, self.name, path
            )?;
        } else {
            writeln!(f, "# {} service definition for {}", self.target, self.name)?;
        }
        if !self.activation_commands.is_empty() {
            writeln!(
                f,
                "
# activation"
            )?;
            for command in &self.activation_commands {
                writeln!(f, "{}", command)?;
            }
        }
        if !self.notes.is_empty() {
            writeln!(
                f,
                "
# notes"
            )?;
            for note in &self.notes {
                writeln!(f, "- {}", note)?;
            }
        }
        write!(f, "{}", self.content)
    }
}

pub fn print_service_definition(
    target: ServiceTarget,
    name: &str,
    workdir: &Path,
    config: Option<&str>,
    gateway_url: Option<&str>,
    yolo: bool,
    no_sandbox: bool,
) -> Result<ServiceDefinitionResponse> {
    let content = render_service_definition(RenderServiceDefinitionOptions {
        target,
        name,
        workdir,
        config,
        gateway_url,
        yolo,
        no_sandbox,
        output_path: None,
    })?;

    Ok(ServiceDefinitionResponse {
        target: service_target_name(target).to_string(),
        name: name.to_string(),
        output_path: None,
        content,
        activation_commands: activation_commands(target, name, None),
        notes: service_notes(target),
    })
}

pub fn install_service_definition(
    options: ServiceInstallOptions,
) -> Result<ServiceDefinitionResponse> {
    let output_path = options
        .output
        .clone()
        .unwrap_or_else(|| default_output_path(options.target, &options.name));
    let content = render_service_definition(RenderServiceDefinitionOptions {
        target: options.target,
        name: &options.name,
        workdir: &options.workdir,
        config: options.config.as_deref(),
        gateway_url: options.gateway_url.as_deref(),
        yolo: options.yolo,
        no_sandbox: options.no_sandbox,
        output_path: Some(&output_path),
    })?;

    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    std::fs::write(&output_path, &content)
        .with_context(|| format!("failed to write {}", output_path.display()))?;

    if options.enable || options.start {
        activate_service(
            options.target,
            &output_path,
            &options.name,
            options.enable,
            options.start,
        )?;
    }

    Ok(ServiceDefinitionResponse {
        target: service_target_name(options.target).to_string(),
        name: options.name.clone(),
        output_path: Some(output_path.display().to_string()),
        content,
        activation_commands: activation_commands(
            options.target,
            &options.name,
            Some(output_path.as_path()),
        ),
        notes: service_notes(options.target),
    })
}

struct RenderServiceDefinitionOptions<'a> {
    target: ServiceTarget,
    name: &'a str,
    workdir: &'a Path,
    config: Option<&'a str>,
    gateway_url: Option<&'a str>,
    yolo: bool,
    no_sandbox: bool,
    output_path: Option<&'a Path>,
}

fn render_service_definition(options: RenderServiceDefinitionOptions<'_>) -> Result<String> {
    let exe = std::env::current_exe().context("failed to resolve current rune binary path")?;
    let mut args = vec!["gateway".to_string(), "run".to_string()];
    if options.yolo {
        args.push("--yolo".to_string());
    }
    if options.no_sandbox {
        args.push("--no-sandbox".to_string());
    }

    Ok(match options.target {
        ServiceTarget::Systemd => render_systemd_unit(
            options.name,
            &exe,
            options.workdir,
            options.config,
            options.gateway_url,
            &args,
            options.output_path,
        ),
        ServiceTarget::Launchd => render_launchd_plist(
            options.name,
            &exe,
            options.workdir,
            options.config,
            options.gateway_url,
            &args,
            options.output_path,
        ),
    })
}

fn render_systemd_unit(
    name: &str,
    exe: &Path,
    workdir: &Path,
    config: Option<&str>,
    gateway_url: Option<&str>,
    args: &[String],
    output_path: Option<&Path>,
) -> String {
    let exec = shell_join(std::iter::once(exe.display().to_string()).chain(args.iter().cloned()));
    let mut unit = format!(
        "[Unit]\nDescription=Rune Gateway ({name})\nAfter=network-online.target\nWants=network-online.target\n\n[Service]\nType=simple\nWorkingDirectory={}\nExecStart={}\nRestart=on-failure\nRestartSec=5\n",
        systemd_escape(workdir),
        systemd_escape_raw(&exec),
    );

    if let Some(config) = config {
        unit.push_str(&format!(
            "Environment=RUNE_CONFIG={}\n",
            systemd_escape_raw(config)
        ));
    }
    if let Some(url) = gateway_url {
        unit.push_str(&format!(
            "Environment=RUNE_GATEWAY_URL={}\n",
            systemd_escape_raw(url)
        ));
    }

    if let Some(path) = output_path {
        unit.push_str("ExecStartPre=/bin/mkdir -p %h/.config/systemd/user\n");
        unit.push_str(&format!(
            "ExecStartPre=/bin/sh -lc 'test -f {}'\n",
            shell_single_quote(&path.display().to_string())
        ));
    }

    unit.push_str("\n[Install]\nWantedBy=default.target\n");
    unit
}

fn render_launchd_plist(
    name: &str,
    exe: &Path,
    workdir: &Path,
    config: Option<&str>,
    gateway_url: Option<&str>,
    args: &[String],
    output_path: Option<&Path>,
) -> String {
    let mut plist = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n",
    );
    plist.push_str(&format!(
        "  <key>Label</key>\n  <string>{}</string>\n",
        xml_escape(name)
    ));
    plist.push_str("  <key>ProgramArguments</key>\n  <array>\n");
    plist.push_str(&format!(
        "    <string>{}</string>\n",
        xml_escape(&exe.display().to_string())
    ));
    for arg in args {
        plist.push_str(&format!("    <string>{}</string>\n", xml_escape(arg)));
    }
    plist.push_str("  </array>\n");
    plist.push_str(&format!(
        "  <key>WorkingDirectory</key>\n  <string>{}</string>\n",
        xml_escape(&workdir.display().to_string())
    ));
    plist.push_str("  <key>RunAtLoad</key>\n  <true/>\n  <key>KeepAlive</key>\n  <true/>\n");

    if config.is_some() || gateway_url.is_some() {
        plist.push_str("  <key>EnvironmentVariables</key>\n  <dict>\n");
        if let Some(config) = config {
            plist.push_str(&format!(
                "    <key>RUNE_CONFIG</key>\n    <string>{}</string>\n",
                xml_escape(config)
            ));
        }
        if let Some(url) = gateway_url {
            plist.push_str(&format!(
                "    <key>RUNE_GATEWAY_URL</key>\n    <string>{}</string>\n",
                xml_escape(url)
            ));
        }
        plist.push_str("  </dict>\n");
    }

    if let Some(path) = output_path {
        plist.push_str(&format!(
            "  <key>StandardOutPath</key>\n  <string>{}</string>\n",
            xml_escape(&path.with_extension("log").display().to_string())
        ));
        plist.push_str(&format!(
            "  <key>StandardErrorPath</key>\n  <string>{}</string>\n",
            xml_escape(&path.with_extension("err.log").display().to_string())
        ));
    }

    plist.push_str("</dict>\n</plist>\n");
    plist
}

fn activation_commands(
    target: ServiceTarget,
    name: &str,
    output_path: Option<&Path>,
) -> Vec<String> {
    match target {
        ServiceTarget::Systemd => {
            let unit = if name.ends_with(".service") {
                name.to_string()
            } else {
                format!("{name}.service")
            };
            let mut commands = vec!["systemctl --user daemon-reload".to_string()];
            if let Some(path) = output_path {
                commands.push(format!(
                    "systemctl --user cat {} # verify installed unit",
                    unit
                ));
                commands.push(format!("systemctl --user enable --now {}", unit));
                commands.push(format!("systemctl --user status {}", unit));
                commands.push(format!("# file: {}", path.display()));
            } else {
                commands.push(format!("systemctl --user enable --now {}", unit));
                commands.push(format!("systemctl --user status {}", unit));
            }
            commands
        }
        ServiceTarget::Launchd => {
            let domain = launchd_domain();
            let service = format!("{domain}/{name}");
            let mut commands = Vec::new();
            if let Some(path) = output_path {
                commands.push(format!("launchctl bootstrap {} {}", domain, path.display()));
            } else {
                commands.push(format!(
                    "launchctl bootstrap {} /path/to/{}.plist",
                    domain, name
                ));
            }
            commands.push(format!("launchctl enable {}", service));
            commands.push(format!("launchctl kickstart -k {}", service));
            commands.push(format!("launchctl print {}", service));
            commands
        }
    }
}

fn service_notes(target: ServiceTarget) -> Vec<String> {
    match target {
        ServiceTarget::Systemd => vec![
            "Use --enable and --start during install to activate automatically.".to_string(),
            "Logs stream via `journalctl --user -u rune-gateway -f` unless you override the service name.".to_string(),
        ],
        ServiceTarget::Launchd => vec![
            "launchd writes stdout/stderr next to the plist when install chooses an output path.".to_string(),
            "Use `launchctl bootout gui/$(id -u)/<label>` before reinstalling if you need a clean reset.".to_string(),
        ],
    }
}

fn activate_service(
    target: ServiceTarget,
    output_path: &Path,
    name: &str,
    enable: bool,
    start: bool,
) -> Result<()> {
    match target {
        ServiceTarget::Systemd => activate_systemd_service(output_path, name, enable, start),
        ServiceTarget::Launchd => activate_launchd_service(output_path, name, enable, start),
    }
}

fn activate_systemd_service(
    output_path: &Path,
    name: &str,
    enable: bool,
    start: bool,
) -> Result<()> {
    run_command(
        std::process::Command::new("systemctl")
            .arg("--user")
            .arg("daemon-reload"),
        "systemctl --user daemon-reload",
    )?;

    if enable {
        let mut cmd = std::process::Command::new("systemctl");
        cmd.arg("--user").arg("enable");
        if start {
            cmd.arg("--now");
        }
        cmd.arg(name);
        run_command(&mut cmd, "systemctl --user enable")?;
    } else if start {
        run_command(
            std::process::Command::new("systemctl")
                .arg("--user")
                .arg("start")
                .arg(name),
            "systemctl --user start",
        )?;
    }

    let _ = output_path;
    Ok(())
}

fn activate_launchd_service(
    output_path: &Path,
    name: &str,
    enable: bool,
    start: bool,
) -> Result<()> {
    if !(enable || start) {
        return Ok(());
    }

    let domain = launchd_domain();
    let service = format!("{domain}/{name}");

    if enable {
        let _ = std::process::Command::new("launchctl")
            .arg("bootout")
            .arg(&service)
            .status();

        run_command(
            std::process::Command::new("launchctl")
                .arg("bootstrap")
                .arg(&domain)
                .arg(output_path),
            "launchctl bootstrap",
        )?;

        run_command(
            std::process::Command::new("launchctl")
                .arg("enable")
                .arg(&service),
            "launchctl enable",
        )?;

        if start {
            run_command(
                std::process::Command::new("launchctl")
                    .arg("kickstart")
                    .arg("-k")
                    .arg(&service),
                "launchctl kickstart -k",
            )?;
        }
        return Ok(());
    }

    run_command(
        std::process::Command::new("launchctl")
            .arg("kickstart")
            .arg("-k")
            .arg(&service),
        "launchctl kickstart -k",
    )?;
    Ok(())
}

fn launchd_domain() -> String {
    match std::env::var("UID") {
        Ok(uid) if !uid.trim().is_empty() => format!("gui/{uid}"),
        _ => "gui/$(id -u)".replace("$(id -u)", &nix_like_uid_fallback()),
    }
}

fn nix_like_uid_fallback() -> String {
    std::process::Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|out| {
            if out.status.success() {
                Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
            } else {
                None
            }
        })
        .filter(|uid| !uid.is_empty())
        .unwrap_or_else(|| "0".to_string())
}

fn run_command(cmd: &mut std::process::Command, display: &str) -> Result<()> {
    let status = cmd
        .status()
        .with_context(|| format!("failed to execute {display}"))?;
    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("{display} exited with status {status}");
    }
}

fn default_output_path(target: ServiceTarget, name: &str) -> PathBuf {
    match target {
        ServiceTarget::Systemd => {
            let base = std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
                .unwrap_or_else(|| PathBuf::from(".config"));
            base.join("systemd/user").join(format!("{name}.service"))
        }
        ServiceTarget::Launchd => {
            let base = std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."));
            base.join("Library/LaunchAgents")
                .join(format!("{name}.plist"))
        }
    }
}

fn service_target_name(target: ServiceTarget) -> &'static str {
    match target {
        ServiceTarget::Systemd => "systemd",
        ServiceTarget::Launchd => "launchd",
    }
}

fn shell_join(parts: impl IntoIterator<Item = String>) -> String {
    parts
        .into_iter()
        .map(|part| {
            if part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.' | ':'))
            {
                part
            } else {
                format!("'{}'", part.replace('\'', "'\\''"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_single_quote(value: &str) -> String {
    value.replace('\'', "'\\''")
}

fn systemd_escape(path: &Path) -> String {
    systemd_escape_raw(&path.display().to_string())
}

fn systemd_escape_raw(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_writes_systemd_unit_to_requested_path() {
        let temp = std::env::temp_dir().join(format!("rune-service-test-{}", std::process::id()));
        std::fs::create_dir_all(&temp).unwrap();
        let output = temp.join("rune-gateway.service");

        let result = install_service_definition(ServiceInstallOptions {
            target: ServiceTarget::Systemd,
            name: "rune-gateway".into(),
            workdir: temp.clone(),
            config: Some("config.toml".into()),
            gateway_url: Some("http://127.0.0.1:8787".into()),
            yolo: true,
            no_sandbox: true,
            output: Some(output.clone()),
            enable: false,
            start: false,
        })
        .unwrap();

        let written = std::fs::read_to_string(&output).unwrap();
        assert_eq!(
            result.output_path.as_deref(),
            Some(output.to_string_lossy().as_ref())
        );
        assert!(written.contains("Description=Rune Gateway (rune-gateway)"));
        assert!(written.contains("Environment=RUNE_CONFIG=config.toml"));
        assert!(written.contains("Environment=RUNE_GATEWAY_URL=http://127.0.0.1:8787"));
        assert!(written.contains("--yolo"));
        assert!(written.contains("--no-sandbox"));
        assert!(written.contains("ExecStartPre=/bin/sh -lc 'test -f "));

        let _ = std::fs::remove_file(output);
        let _ = std::fs::remove_dir_all(temp);
    }

    #[test]
    fn print_launchd_plist_embeds_program_arguments() {
        let temp = std::env::temp_dir().join(format!("rune-service-test-{}", std::process::id()));
        let result = print_service_definition(
            ServiceTarget::Launchd,
            "rune-gateway",
            &temp,
            Some("config.toml"),
            Some("http://127.0.0.1:8787"),
            false,
            false,
        )
        .unwrap();

        assert_eq!(result.target, "launchd");
        assert!(result.output_path.is_none());
        assert!(
            result
                .content
                .contains("<key>Label</key>\n  <string>rune-gateway</string>")
        );
        assert!(result.content.contains("<key>ProgramArguments</key>"));
        assert!(result.content.contains("<string>gateway</string>"));
        assert!(result.content.contains("<string>run</string>"));
        assert!(result.content.contains("<key>RUNE_CONFIG</key>"));
        assert!(result.content.contains("<string>config.toml</string>"));
    }

    #[test]
    fn install_launchd_writes_log_paths_next_to_plist() {
        let temp = tempfile::tempdir().unwrap();
        let output = temp.path().join("rune-gateway.plist");

        let result = install_service_definition(ServiceInstallOptions {
            target: ServiceTarget::Launchd,
            name: "rune-gateway".into(),
            workdir: temp.path().to_path_buf(),
            config: None,
            gateway_url: None,
            yolo: false,
            no_sandbox: false,
            output: Some(output.clone()),
            enable: false,
            start: false,
        })
        .unwrap();

        let written = std::fs::read_to_string(&output).unwrap();
        assert_eq!(
            result.output_path.as_deref(),
            Some(output.to_string_lossy().as_ref())
        );
        assert!(written.contains("<key>StandardOutPath</key>"));
        assert!(written.contains(&output.with_extension("log").display().to_string()));
        assert!(written.contains("<key>StandardErrorPath</key>"));
        assert!(written.contains(&output.with_extension("err.log").display().to_string()));
    }
}
