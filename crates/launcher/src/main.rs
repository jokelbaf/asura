mod application;
mod prefix;

use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, ExitCode};

use anyhow::bail;
use capture::Scope;
use clap::Parser;
use control::{Reply, Request};

#[derive(Parser)]
#[command(
    name = "asura-run",
    about = "Run a program with its traffic captured by Asura"
)]
struct Options {
    #[arg(long, help = "Name shown for this target in Asura")]
    name: Option<String>,

    #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
    command: Vec<String>,
}

fn main() -> ExitCode {
    let options = Options::parse();

    if let Err(error) = prepare(&options) {
        eprintln!("asura-run: running without capture: {error:#}");
    }

    let error = Command::new(&options.command[0])
        .args(&options.command[1..])
        .exec();
    eprintln!("asura-run: cannot execute {}: {error}", options.command[0]);
    ExitCode::FAILURE
}

fn prepare(options: &Options) -> anyhow::Result<()> {
    application::ensure()?;

    let name = options
        .name
        .clone()
        .unwrap_or_else(|| guess(&options.command));
    let mut processes = vec![std::process::id()];

    if let Some(prefix) = prefix::detect(&options.command) {
        match prefix.wineserver() {
            Some(server) => {
                eprintln!(
                    "asura-run: wineserver {server} of {} joins the capture",
                    prefix.path.display()
                );
                processes.push(server);
            }
            None => prefix::trust(&prefix),
        }
    }

    Scope::create(&name, &processes)?;

    let request = Request::Register {
        name: name.clone(),
        process: processes[0],
        command: options.command.clone(),
    };

    match control::send(&request)? {
        Reply::Registered { capturing: true } => eprintln!("asura-run: capturing {name}"),
        Reply::Registered { capturing: false } => {
            eprintln!("asura-run: {name} registered, capture is stopped")
        }
        Reply::Failed(reason) => bail!(reason),
    }

    Ok(())
}

fn guess(command: &[String]) -> String {
    let executable = command
        .iter()
        .rev()
        .find(|argument| argument.to_ascii_lowercase().ends_with(".exe"))
        .unwrap_or(&command[0]);

    let name = executable.rsplit(['/', '\\']).next().unwrap_or(executable);

    Path::new(name).file_stem().map_or_else(
        || name.to_owned(),
        |stem| stem.to_string_lossy().into_owned(),
    )
}
