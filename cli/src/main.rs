use std::{
    path::PathBuf,
    process,
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};

use clap::Parser;
use dialoguer::Confirm;
use indicatif::{HumanBytes, HumanDuration, ProgressBar, ProgressStyle};
use lessanvil::Config;
use owo_colors::OwoColorize;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    world_folder: PathBuf,
    /// The maximum amount of time players can have spent spent in a chunk for it to get
    /// remmoved in seconds. See https://minecraft.fandom.com/wiki/Chunk_format#NBT_structure
    #[arg(short, long, default_value = "0")]
    max_inhabited_time: usize,
    /// The amount of threads spawned. Default is the same as the number of CPUs available
    #[arg(short, long)]
    thread_count: Option<usize>,
    /// Skip confirmation prompt. Use this with caution!
    #[arg(long, default_value = "false")]
    confirm: bool,
    /// Skip all checks for the world being valid. Use this with caution!
    #[arg(long, default_value = "false")]
    force: bool,
    /// Whether the final report should be in json
    #[arg(long, default_value = "false")]
    json: bool,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
enum ProcessingUpdate {
    Processing { progress: f64 },
    Finished { report: CliReport },
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliReport {
    pub time_taken: Duration,
    pub total_freed_space: u64,
    pub total_regions: u64,
    pub total_chunks: u64,
    pub total_deleted_chunks: u64,
}

fn main() {
    env_logger::init();

    let args = Args::parse();

    // Check if valid world
    if !args.force && !args.world_folder.join("level.dat").exists()
        || !args.world_folder.join("region").exists()
    {
        log::error!("Invalid world folder!");
        process::exit(1);
    }

    if !args.confirm {
        anstream::eprintln!("This tool will remove all chunks in which players have been less than the given amount of time.");
        anstream::eprintln!("{}: This tool will work on the given world folder. Therefore it's recommended to {} before continuing.", "Warning".black().on_red().bold(), "create a backup".black().on_yellow().bold());
        if !Confirm::new()
            .with_prompt("Do you want to continue?")
            .interact()
            .unwrap()
        {
            anstream::eprintln!("Aborting.");
            process::exit(1);
        }
    }

    let config = Config {
        world_folder: args.world_folder,
        max_inhabited_time: args.max_inhabited_time,
        thread_count: args.thread_count.unwrap_or(num_cpus::get()),
    };

    let progress_bar = if args.json {
        ProgressBar::hidden()
    } else {
        ProgressBar::new(0).with_style(
            ProgressStyle::with_template(
                "Processing files: {pos}/{len} files | {per_sec} [{wide_bar:0.yellow}] {percent}% | {elapsed} ",
            )
            .unwrap()
            .progress_chars("#> ")
        )
    };

    let rx = match lessanvil::execute(config) {
        Ok(rx) => rx,
        Err(err) => {
            log::error!("{}", err);
            process::exit(1)
        }
    };

    let mut total_items = 1;
    let mut processed_items = 0;

    let running = Arc::new(AtomicBool::new(true));

    let r = running.clone();
    let _ = ctrlc::set_handler(move || r.store(false, std::sync::atomic::Ordering::Relaxed));

    loop {
        if let Ok(msg) = rx.recv() {
            match msg {
                lessanvil::ProcessingUpdate::Starting { total_files } => {
                    total_items = total_files;
                    progress_bar.set_length(total_files)
                }
                lessanvil::ProcessingUpdate::ProcessedRegion(_) => {
                    progress_bar.inc(1);

                    if args.json {
                        processed_items += 1;
                        anstream::println!(
                            "{}",
                            serde_json::to_string(&ProcessingUpdate::Processing {
                                progress: processed_items as f64 / total_items as f64,
                            })
                            .unwrap()
                        );
                    }
                }
                lessanvil::ProcessingUpdate::Finished(report) => {
                    anstream::println!(
                        "{}",
                        if args.json {
                            serde_json::to_string(&ProcessingUpdate::Finished {
                                report: CliReport {
                                    time_taken: report.time_taken,
                                    total_freed_space: report.total_freed_space,
                                    total_regions: report.total_regions,
                                    total_chunks: report.total_chunks,
                                    total_deleted_chunks: report.total_deleted_chunks,
                                },
                            })
                            .unwrap()
                        } else {
                            format!(
                                "Successfully processed {} files in {} and freed up {} by deleting {} chunks.",
                                report.total_regions.yellow(),
                                HumanDuration(report.time_taken).yellow(),
                                HumanBytes(report.total_freed_space).yellow(),
                                report.total_deleted_chunks.yellow()
                            )
                        },
                    );
                    process::exit(0)
                }
            }
        }

        if !running.load(std::sync::atomic::Ordering::Relaxed) {
            anstream::eprintln!("Aborting.");
            drop(rx);
            return;
        }
    }
}
