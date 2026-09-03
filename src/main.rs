use std::{
    io::{self, BufWriter},
    path::{Path, PathBuf},
    process::ExitCode,
};

use clap::{Parser, Subcommand, ValueEnum};
use fsproof::{
    BackendMode, DEFAULT_MAX_BYTES, DEFAULT_MAX_ENTRIES, FsError, ScanLimits, WatchOptions,
    inspect, parse_duration, render_inspect_text, render_json, render_verify_text, verify, watch,
};

#[derive(Debug, Parser)]
#[command(
    name = "fsproof",
    version,
    about = "Reconcile filesystem notifications with bounded snapshots"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Inspect {
        path: PathBuf,
        #[arg(long, default_value_t = DEFAULT_MAX_ENTRIES)]
        max_entries: usize,
        #[arg(long, default_value_t = DEFAULT_MAX_BYTES)]
        max_bytes: u64,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    Verify {
        path: PathBuf,
        #[arg(long, default_value = "10s")]
        duration: String,
        #[arg(long, value_enum, default_value_t = BackendArg::Native)]
        backend: BackendArg,
        #[arg(long, default_value = "500ms")]
        poll_interval: String,
        #[arg(long, default_value_t = DEFAULT_MAX_ENTRIES)]
        max_entries: usize,
        #[arg(long, default_value_t = DEFAULT_MAX_BYTES)]
        max_bytes: u64,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    Watch {
        path: PathBuf,
        #[arg(long)]
        duration: Option<String>,
        #[arg(long, value_enum, default_value_t = BackendArg::Native)]
        backend: BackendArg,
        #[arg(long, default_value = "500ms")]
        poll_interval: String,
        #[arg(long, default_value_t = DEFAULT_MAX_ENTRIES)]
        max_entries: usize,
        #[arg(long, default_value_t = DEFAULT_MAX_BYTES)]
        max_bytes: u64,
        #[arg(long, value_enum, default_value_t = WatchFormat::Jsonl)]
        format: WatchFormat,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum BackendArg {
    Native,
    Poll,
}

impl From<BackendArg> for BackendMode {
    fn from(value: BackendArg) -> Self {
        match value {
            BackendArg::Native => Self::Native,
            BackendArg::Poll => Self::Poll,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum OutputFormat {
    Text,
    Json,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum WatchFormat {
    Jsonl,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Inspect {
            path,
            max_entries,
            max_bytes,
            format,
        } => match inspect(&path, &limits(max_entries, max_bytes)) {
            Ok(report) => finish_inspect(&report, format),
            Err(error) => finish_error(error),
        },
        Command::Verify {
            path,
            duration,
            backend,
            poll_interval,
            max_entries,
            max_bytes,
            format,
        } => finish_verify(
            &path,
            &duration,
            backend,
            &poll_interval,
            max_entries,
            max_bytes,
            format,
        ),
        Command::Watch {
            path,
            duration,
            backend,
            poll_interval,
            max_entries,
            max_bytes,
            format: WatchFormat::Jsonl,
        } => finish_watch(
            &path,
            duration.as_deref(),
            backend,
            &poll_interval,
            max_entries,
            max_bytes,
        ),
    }
}

fn finish_inspect(report: &fsproof::InspectReport, format: OutputFormat) -> ExitCode {
    match format {
        OutputFormat::Text => print!("{}", render_inspect_text(report)),
        OutputFormat::Json => match render_json(report) {
            Ok(value) => println!("{value}"),
            Err(error) => return finish_render_error(error),
        },
    }
    ExitCode::from(report.exit_code() as u8)
}

fn finish_verify(
    path: &Path,
    duration: &str,
    backend: BackendArg,
    poll_interval: &str,
    max_entries: usize,
    max_bytes: u64,
    format: OutputFormat,
) -> ExitCode {
    let duration = match parse_duration(duration) {
        Ok(value) => value,
        Err(error) => return finish_error(error),
    };
    let poll_interval = match parse_duration(poll_interval) {
        Ok(value) if !value.is_zero() => value,
        Ok(_) => return finish_error(FsError::new("poll interval must be greater than zero")),
        Err(error) => return finish_error(error),
    };
    let options = WatchOptions {
        backend: backend.into(),
        poll_interval,
        limits: limits(max_entries, max_bytes),
        ..WatchOptions::default()
    };
    match verify(path, duration, &options) {
        Ok(report) => match format {
            OutputFormat::Text => {
                print!("{}", render_verify_text(&report));
                ExitCode::from(report.exit_code() as u8)
            }
            OutputFormat::Json => match render_json(&report) {
                Ok(value) => {
                    println!("{value}");
                    ExitCode::from(report.exit_code() as u8)
                }
                Err(error) => finish_render_error(error),
            },
        },
        Err(error) => finish_error(error),
    }
}

fn finish_watch(
    path: &Path,
    duration: Option<&str>,
    backend: BackendArg,
    poll_interval: &str,
    max_entries: usize,
    max_bytes: u64,
) -> ExitCode {
    let duration = match duration.map(parse_duration).transpose() {
        Ok(value) => value,
        Err(error) => return finish_error(error),
    };
    let poll_interval = match parse_duration(poll_interval) {
        Ok(value) if !value.is_zero() => value,
        Ok(_) => return finish_error(FsError::new("poll interval must be greater than zero")),
        Err(error) => return finish_error(error),
    };
    let options = WatchOptions {
        backend: backend.into(),
        duration,
        poll_interval,
        limits: limits(max_entries, max_bytes),
    };
    let stdout = io::stdout();
    let mut writer = BufWriter::new(stdout.lock());
    match watch(path, &options, &mut writer) {
        Ok(summary) => ExitCode::from(summary.exit_code() as u8),
        Err(error) => finish_error(error),
    }
}

fn limits(max_entries: usize, max_bytes: u64) -> ScanLimits {
    ScanLimits {
        max_entries,
        max_bytes,
    }
}

fn finish_error(error: FsError) -> ExitCode {
    eprintln!("fsproof: {error}");
    ExitCode::from(3)
}

fn finish_render_error(error: serde_json::Error) -> ExitCode {
    eprintln!("fsproof: could not render report: {error}");
    ExitCode::from(3)
}
