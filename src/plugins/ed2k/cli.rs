use crate::plugins::registry::{CliPlugin, DownloadCliConfig};
use clap::{Arg, ArgAction, ArgMatches, Command};

pub struct Ed2kCliPlugin;

impl Ed2kCliPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl CliPlugin for Ed2kCliPlugin {
    fn name(&self) -> &'static str {
        "ed2k"
    }

    fn augment_download_command(&self, cmd: Command) -> Command {
        cmd.arg(
            Arg::new("ed2k_cmd")
                .long("ed2k-cmd")
                .help_heading("ED2K")
                .help("External ED2K downloader command to run. If set, it will be executed with --ed2k-arg arguments. Supports placeholders: {url} {out} {name} {size} {hash}")
                .num_args(1),
        )
        .arg(
            Arg::new("ed2k_arg")
                .long("ed2k-arg")
                .help_heading("ED2K")
                .help("Argument passed to --ed2k-cmd (repeatable). Placeholders supported: {url} {out} {name} {size} {hash}")
                .action(ArgAction::Append)
                .num_args(1),
        )
    }

    fn apply_download_matches(&self, matches: &ArgMatches, cfg: &mut DownloadCliConfig) -> anyhow::Result<()> {
        if let Some(v) = matches.get_one::<String>("ed2k_cmd") {
            cfg.options.insert("ed2k_cmd".to_string(), v.clone());
        }
        if let Some(vs) = matches.get_many::<String>("ed2k_arg") {
            let joined = vs.map(|s| s.as_str()).collect::<Vec<_>>().join("\n");
            cfg.options.insert("ed2k_args".to_string(), joined);
        }
        Ok(())
    }
}
