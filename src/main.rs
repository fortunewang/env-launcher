use anyhow::Context;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{self, Command, Stdio};

trait ToOsString {
    fn to_os_string(&self) -> OsString;
}

impl<T: AsRef<OsStr>> ToOsString for T {
    #[inline]
    fn to_os_string(&self) -> OsString {
        self.as_ref().to_os_string()
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum EnvConfig {
    Simple(String),
    Detailed {
        #[serde(default)]
        append: Vec<String>,
        #[serde(default)]
        prepend: Vec<String>,
        sep: String,
    },
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Config {
    command: PathBuf,
    args: Vec<String>,
    env: BTreeMap<String, EnvConfig>,
    detach: bool,
}

// https://learn.microsoft.com/en-us/windows/win32/procthread/process-creation-flags
const DETACHED_PROCESS: u32 = 0x00000008;

fn parse_args() -> clap::ArgMatches {
    clap::Command::new("launcher")
        .args(&[
            clap::Arg::new("config")
                .long("config")
                .short('c')
                .value_parser(clap::value_parser!(PathBuf)),
            clap::Arg::new("env")
                .long("env")
                .short('e')
                .action(clap::ArgAction::Append),
            clap::Arg::new("detach")
                .long("detach")
                .short('d')
                .action(clap::ArgAction::SetTrue),
            clap::Arg::new("command").value_parser(clap::value_parser!(PathBuf)),
            clap::Arg::new("arg").action(clap::ArgAction::Append),
        ])
        .get_matches()
}

fn load_config<P: AsRef<Path>>(config_path: P) -> anyhow::Result<Config> {
    let config_path = config_path.as_ref();
    let config_content = std::fs::read(config_path)
        .with_context(|| format!("read config file {}", config_path.display()))?;
    toml::from_slice(&config_content)
        .with_context(|| format!("parse config file {}", config_path.display()))
}

fn override_config_with_args(config: &mut Config, args: &clap::ArgMatches) {
    if let Some(command) = args.get_one::<PathBuf>("command") {
        config.command = command.clone();
        config.args = Vec::new();
        if let Some(command_args) = args.get_many::<String>("arg") {
            for arg in command_args {
                config.args.push(arg.clone())
            }
        }
    }
    if let Some(envs) = args.get_many::<String>("env") {
        for env in envs {
            if let Some((env_name, env_value)) = env.split_once('=') {
                config.env.insert(
                    env_name.to_string(),
                    EnvConfig::Simple(env_value.to_string()),
                );
            }
        }
    }
    if args.get_flag("detach") {
        config.detach = true;
    }
}

fn main() -> anyhow::Result<()> {
    let args = parse_args();
    let mut config = if let Some(path) = args.get_one::<PathBuf>("config") {
        load_config(path)?
    } else {
        let launcher_path = std::env::current_exe().context("get aluncher path")?;
        let config_path = launcher_path.with_extension("toml");
        if config_path.exists() {
            load_config(config_path)?
        } else {
            Config::default()
        }
    };

    override_config_with_args(&mut config, &args);

    if config.command.to_string_lossy().is_empty() {
        anyhow::bail!("command not specified")
    }

    let mut command = Command::new(&config.command);
    if !config.args.is_empty() {
        command.args(config.args);
    }
    for (env_name, env) in config.env {
        match env {
            EnvConfig::Simple(value) => {
                command.env(env_name, value);
            }
            EnvConfig::Detailed {
                append,
                prepend,
                sep,
            } => {
                let prepend = prepend.join(&sep).to_os_string();
                let append = append.join(&sep).to_os_string();
                let origin = std::env::var_os(&env_name).unwrap_or_default();
                let mut value = prepend;
                if !origin.is_empty() {
                    if !value.is_empty() {
                        value.push(&sep);
                    }
                    value.push(&origin);
                }
                if !append.is_empty() {
                    if !value.is_empty() {
                        value.push(&sep);
                    }
                    value.push(&append);
                }
                command.env(env_name, value);
            }
        }
    }

    if config.detach {
        command.stdin(Stdio::null());
        command.stdout(Stdio::null());
        command.stderr(Stdio::null());
        command.creation_flags(DETACHED_PROCESS);
    }
    let mut child = command
        .spawn()
        .with_context(|| format!("spawn process {}", config.command.display()))?;

    if !config.detach {
        let status = child.wait().context("wait for child process")?;
        if !status.success() {
            process::exit(status.code().unwrap_or(-1));
        }
    }

    Ok(())
}
