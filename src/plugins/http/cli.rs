use crate::plugins::registry::{CliPlugin, DownloadCliConfig};
use clap::{Arg, ArgAction, ArgMatches, Command};

pub struct HttpCliPlugin;

impl HttpCliPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl CliPlugin for HttpCliPlugin {
    fn name(&self) -> &'static str {
        "http"
    }

    fn augment_download_command(&self, cmd: Command) -> Command {
        cmd.arg(
            Arg::new("http_header")
                .long("header")
                .help_heading("HTTP")
                .help("Extra HTTP header (repeatable), e.g. --header 'Authorization: Bearer xxx'")
                .action(ArgAction::Append)
                .num_args(1),
        )
        .arg(
            Arg::new("http_user_agent")
                .long("user-agent")
                .help_heading("HTTP")
                .help("HTTP User-Agent")
                .default_value("OrangeDownloader/0.1")
                .num_args(1),
        )
        .arg(
            Arg::new("http_timeout_secs")
                .long("timeout-secs")
                .help_heading("HTTP")
                .help("HTTP timeout in seconds")
                .default_value("60")
                .num_args(1),
        )
        .arg(
            Arg::new("http_retries")
                .long("retries")
                .help_heading("HTTP")
                .help("HTTP retries for transient errors")
                .default_value("2")
                .num_args(1),
        )
        .arg(
            Arg::new("http_retry_backoff_ms")
                .long("retry-backoff-ms")
                .help_heading("HTTP")
                .help("Retry backoff base in milliseconds")
                .default_value("400")
                .num_args(1),
        )
    }

    fn apply_download_matches(&self, matches: &ArgMatches, cfg: &mut DownloadCliConfig) -> anyhow::Result<()> {
        if let Some(ua) = matches.get_one::<String>("http_user_agent") {
            cfg.driver_ctx.user_agent = ua.clone();
        }
        if let Some(s) = matches.get_one::<String>("http_timeout_secs") {
            cfg.driver_ctx.timeout_secs = s.parse()?;
        }
        if let Some(s) = matches.get_one::<String>("http_retries") {
            cfg.driver_ctx.retries = s.parse()?;
        }
        if let Some(s) = matches.get_one::<String>("http_retry_backoff_ms") {
            cfg.driver_ctx.retry_backoff_ms = s.parse()?;
        }

        if let Some(values) = matches.get_many::<String>("http_header") {
            for h in values {
                let (k, v) = h
                    .split_once(':')
                    .ok_or_else(|| anyhow::anyhow!("invalid header format: {}", h))?;
                cfg.headers.insert(k.trim().to_string(), v.trim().to_string());
            }
        }

        Ok(())
    }
}
