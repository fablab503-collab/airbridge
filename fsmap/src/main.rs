use clap::{Parser, Subcommand};
use fsmap::{html, scan, terminal, util, volumes};
use std::path::PathBuf;
use std::process::ExitCode;

/// fsmap: a strictly read-only map of your disks and folders.
#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// List mounted disks and shares, and explain each filesystem type in plain words.
    Volumes {
        /// Also show system and pseudo filesystems.
        #[arg(long)]
        all: bool,
        /// Write the full list as JSON to this file.
        #[arg(long, value_name = "FILE")]
        json: Option<PathBuf>,
    },
    /// Walk folders (read-only) and report what is filling them.
    Scan {
        /// Folders to scan.
        #[arg(required = true, value_name = "PATH")]
        paths: Vec<PathBuf>,
        /// How many folder levels to rank.
        #[arg(long, default_value_t = 2)]
        depth: usize,
        /// How many items to keep in each "biggest" list.
        #[arg(long, default_value_t = 25)]
        top: usize,
        /// Look for duplicate files (reads file contents).
        #[arg(long)]
        dupes: bool,
        /// Ignore files smaller than this when looking for duplicates (e.g. 1MiB, 500k, 2G).
        #[arg(long, default_value = "1MiB", value_parser = util::parse_size)]
        min_dupe_size: u64,
        /// Also walk into other disks/shares mounted inside these folders.
        #[arg(long)]
        cross_filesystems: bool,
        /// Write everything as JSON to this file.
        #[arg(long, value_name = "FILE")]
        json: Option<PathBuf>,
        /// Write a one-page offline HTML report to this file.
        #[arg(long, value_name = "FILE")]
        html: Option<PathBuf>,
        /// Worker threads (0 = automatic).
        #[arg(long, default_value_t = 0)]
        threads: usize,
        /// Do not show live progress on stderr.
        #[arg(long)]
        no_progress: bool,
    },
}

fn write_report(path: &PathBuf, text: &str) -> Result<(), String> {
    std::fs::write(path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    eprintln!("wrote {}", path.display());
    Ok(())
}

fn run() -> Result<(), String> {
    match Cli::parse().cmd {
        Cmd::Volumes { all, json } => {
            let vols = volumes::list(all).map_err(|e| format!("cannot list volumes: {e}"))?;
            print!("{}", terminal::volumes_text(&vols));
            if let Some(j) = json {
                write_report(
                    &j,
                    &serde_json::to_string_pretty(&vols).map_err(|e| e.to_string())?,
                )?;
            }
        }
        Cmd::Scan {
            paths,
            depth,
            top,
            dupes,
            min_dupe_size,
            cross_filesystems,
            json,
            html: html_out,
            threads,
            no_progress,
        } => {
            let opts = scan::ScanOptions {
                depth: depth.max(1),
                top,
                dupes,
                min_dupe_size,
                cross_filesystems,
                threads,
                progress: !no_progress,
            };
            let report = scan::scan(&paths, &opts)?;
            print!("{}", terminal::scan_text(&report));
            if let Some(j) = json {
                write_report(
                    &j,
                    &serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?,
                )?;
            }
            if let Some(h) = html_out {
                write_report(&h, &html::render(&report))?;
            }
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("fsmap: {e}");
            ExitCode::FAILURE
        }
    }
}
