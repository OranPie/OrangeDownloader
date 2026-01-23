use crate::plugins::registry::{CliPlugin, DownloadCliConfig};
use clap::{Arg, ArgMatches, Command};

pub struct FtpCliPlugin;

impl FtpCliPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl CliPlugin for FtpCliPlugin {
    fn name(&self) -> &'static str {
        "ftp"
    }

    fn augment_download_command(&self, cmd: Command) -> Command {
        cmd.arg(
            Arg::new("ftp_user")
                .long("ftp-user")
                .help_heading("FTP")
                .help("FTP username (default: anonymous)")
                .num_args(1),
        )
        .arg(
            Arg::new("ftp_pass")
                .long("ftp-pass")
                .help_heading("FTP")
                .help("FTP password")
                .num_args(1),
        )
        .arg(
            Arg::new("ftp_port")
                .long("ftp-port")
                .help_heading("FTP")
                .help("FTP port")
                .default_value("21")
                .num_args(1),
        )
    }

    fn apply_download_matches(&self, matches: &ArgMatches, cfg: &mut DownloadCliConfig) -> anyhow::Result<()> {
        if let Some(v) = matches.get_one::<String>("ftp_user") {
            cfg.options.insert("ftp_user".to_string(), v.clone());
        }
        if let Some(v) = matches.get_one::<String>("ftp_pass") {
            cfg.options.insert("ftp_pass".to_string(), v.clone());
        }
        if let Some(v) = matches.get_one::<String>("ftp_port") {
            cfg.options.insert("ftp_port".to_string(), v.clone());
        }
        Ok(())
    }
}
