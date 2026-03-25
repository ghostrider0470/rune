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
    let content =
        render_service_definition(target, name, workdir, config, gateway_url, yolo, no_sandbox)?;

    Ok(ServiceDefinitionResponse {
        target: service_target_name(target).to_string(),
        name: name.to_string(),
        output_path: None,
        content,
    })
}

pub fn install_service_definition(
    options: ServiceInstallOptions,
) -> Result<ServiceDefinitionResponse> {
    let content = render_service_definition(
        options.target,
        &options.name,
        &options.workdir,
        options.config.as_deref(),
        options.gateway_url.as_deref(),
        options.yolo,
        options.no_sandbox,
    )?;

    let output_path = options
        .output
        .unwrap_or_else(|| default_output_path(options.target, &options.name));
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
        name: options.name,
        output_path: Some(output_path.display().to_string()),
        content,
    })
}

fn render_service_definition(
    target: ServiceTarget,
    name: &str,
    workdir: &Path,
    config: Option<&str>,
    gateway_url: Option<&str>,
    yolo: bool,
    no_sandbox: bool,
) -> Result<String> {
    let exe = std::env::current_exe().context("failed to resolve current rune binary path")?;
    let mut args = vec!["gateway".to_string(), "run".to_string()];
    if yolo {
        args.push("--yolo".to_string());
    }
    if no_sandbox {
        args.push("--no-sandbox".to_string());
    }

    Ok(match target {
        ServiceTarget::Systemd => {
            render_systemd_unit(name, &exe, workdir, config, gateway_url, &args)
        }
        ServiceTarget::Launchd => {
            render_launchd_plist(name, &exe, workdir, config, gateway_url, &args)
        }
    })
}

fn render_systemd_unit(
    name: &str,
    exe: &Path,
    workdir: &Path,
    config: Option<&str>,
    gateway_url: Option<&str>,
    args: &[String],
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

    plist.push_str("</dict>\n</plist>\n");
    plist
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
    _name: &str,
    enable: bool,
    start: bool,
) -> Result<()> {
    if enable || start {
        run_command(
            std::process::Command::new("launchctl")
                .arg("load")
                .arg("-w")
                .arg(output_path),
            "launchctl load -w",
        )?;
    }
    Ok(())
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
        assert!(result.content.contains(
            "<key>Label</key>
  <string>rune-gateway</string>"
        ));
        assert!(result.content.contains("<key>ProgramArguments</key>"));
        assert!(result.content.contains("<string>gateway</string>"));
        assert!(result.content.contains("<string>run</string>"));
        assert!(result.content.contains("<key>RUNE_CONFIG</key>"));
        assert!(result.content.contains("<string>config.toml</string>"));
    }
}
