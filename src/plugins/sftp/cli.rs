use crate::plugins::registry::{CliPlugin, DownloadCliConfig};
use clap::{Arg, ArgMatches, Command};

pub struct SftpCliPlugin;

impl SftpCliPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl CliPlugin for SftpCliPlugin {
    fn name(&self) -> &'static str {
        "sftp"
    }

    fn augment_download_command(&self, cmd: Command) -> Command {
        cmd.arg(
            Arg::new("sftp_user")
                .long("sftp-user")
                .help_heading("SFTP")
                .help("SFTP username (if not provided, uses user from URL if present)")
                .num_args(1),
        )
        .arg(
            Arg::new("sftp_port")
                .long("sftp-port")
                .help_heading("SFTP")
                .help("SFTP port")
                .default_value("22")
                .num_args(1),
        )
        .arg(
            Arg::new("sftp_identity")
                .long("sftp-identity")
                .help_heading("SFTP")
                .help("Path to SSH private key file (used by scp -i)")
                .num_args(1),
        )
    }

    fn apply_download_matches(&self, matches: &ArgMatches, cfg: &mut DownloadCliConfig) -> anyhow::Result<()> {
        if let Some(v) = matches.get_one::<String>("sftp_user") {
            cfg.options.insert("sftp_user".to_string(), v.clone());
        }
        if let Some(v) = matches.get_one::<String>("sftp_port") {
            cfg.options.insert("sftp_port".to_string(), v.clone());
        }
        if let Some(v) = matches.get_one::<String>("sftp_identity") {
            cfg.options.insert("sftp_identity".to_string(), v.clone());
        }
        Ok(())
    }
}
