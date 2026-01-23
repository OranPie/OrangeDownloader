use crate::plugins::registry::{CliPlugin, DownloadCliConfig};
use clap::{Arg, ArgMatches, Command};

pub struct AdbCliPlugin;

impl AdbCliPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl CliPlugin for AdbCliPlugin {
    fn name(&self) -> &'static str {
        "adb"
    }

    fn augment_download_command(&self, cmd: Command) -> Command {
        cmd.arg(
            Arg::new("adb_serial")
                .long("adb-serial")
                .help_heading("ANDROID")
                .help("ADB device serial (passed to adb -s)")
                .num_args(1),
        )
        .arg(
            Arg::new("adb_bin")
                .long("adb-bin")
                .help_heading("ANDROID")
                .help("Path to adb binary")
                .default_value("adb")
                .num_args(1),
        )
    }

    fn apply_download_matches(&self, matches: &ArgMatches, cfg: &mut DownloadCliConfig) -> anyhow::Result<()> {
        if let Some(v) = matches.get_one::<String>("adb_serial") {
            cfg.options.insert("adb_serial".to_string(), v.clone());
        }
        if let Some(v) = matches.get_one::<String>("adb_bin") {
            cfg.options.insert("adb_bin".to_string(), v.clone());
        }
        Ok(())
    }
}
