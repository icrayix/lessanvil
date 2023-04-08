use std::{
    fs::{self, File},
    io::{self, Seek},
    path::{Path, PathBuf},
    process,
    sync::atomic::AtomicU64,
    time,
};

use anyhow::bail;
use clap::Parser;
use dialoguer::Confirm;
use fastanvil::Region;
use indicatif::{HumanBytes, HumanDuration, ParallelProgressIterator, ProgressBar, ProgressStyle};
use owo_colors::OwoColorize;
use rayon::{
    prelude::{IntoParallelIterator, ParallelIterator},
    ThreadPoolBuilder,
};
use serde::{Deserialize, Serialize};

/// The subfolders in the world folder in which the region files are contained
const REGION_SUBFOLDERS: [&str; 3] = ["region", "DIM-1/region", "DIM1/region"];

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
    /// Whether the final report should be in json
    #[arg(long, default_value = "false")]
    json: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Chunk {
    inhabited_time: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Report {
    total_time_in_seconds: u64,
    total_processed_files: usize,
    total_deleted_chunks: u64,
    total_freed_space_in_kib: u64,
}

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let args = Args::parse();
    if !args.world_folder.exists() {
        bail!("Specified folder doesnt exist.");
    }

    // Check if valid world
    if !args.world_folder.join("level.dat").exists() || !args.world_folder.join("region").exists() {
        log::error!("Invalid world!");
        process::exit(1);
    }

    if !args.confirm {
        anstream::eprintln!("This tool will remove all chunks in which players have been less than the given amount of time.");
        anstream::eprintln!("{}: This tool will work on the given world folder. Therefore it's recommended to {} before continuing.", "Warning".black().on_red().bold(), "create a backup".black().on_yellow().bold());
        if !Confirm::new()
            .with_prompt("Do you want to continue?")
            .interact()?
        {
            anstream::eprintln!("Aborting.");
            process::exit(1);
        }
    }

    if let Some(threads) = args.thread_count {
        ThreadPoolBuilder::new()
            .num_threads(threads)
            .build_global()?;
    }

    let size_before = dir_size(args.world_folder.as_path())?;
    let start_time = time::Instant::now();

    let files = match collect_region_files(Path::new(&args.world_folder)) {
        Ok(files) => files,
        Err(err) => {
            log::error!("Failed to collect region files: {}", err);
            process::exit(1)
        }
    };

    let total_files = files.len();
    let total_deleted_chunks = AtomicU64::new(0);

    let progress_bar = ProgressBar::new(files.len() as u64).with_style(
        ProgressStyle::with_template(
            "Processing files: {pos}/{len} files | {per_sec} [{wide_bar:0.yellow}] {percent}% | {elapsed} ",
        )?
        .progress_chars("#> ")
    );

    files
        .into_par_iter()
        .progress_with(progress_bar)
        .for_each(
            |path| match process_region_file(path.as_path(), args.max_inhabited_time * 20) {
                Ok(deleted_chunks) => {
                    total_deleted_chunks
                        .fetch_add(deleted_chunks as u64, std::sync::atomic::Ordering::Relaxed);
                }
                Err(err) => log::error!(
                    "Failed to process region file ({}): {}",
                    path.display(),
                    err
                ),
            },
        );

    let freed_space = size_before - dir_size(args.world_folder.as_path())?;
    let time_taken = time::Instant::now() - start_time;

    let report = if args.json {
        serde_json::to_string(&Report {
            total_time_in_seconds: time_taken.as_secs(),
            total_processed_files: total_files,
            total_deleted_chunks: total_deleted_chunks.into_inner(),
            total_freed_space_in_kib: freed_space / 1024,
        })
        .unwrap()
    } else {
        format!(
            "Successfully processed {} files in {} and freed up {} by deleting {} chunks.",
            total_files.yellow(),
            HumanDuration(time_taken).yellow(),
            HumanBytes(freed_space).yellow(),
            total_deleted_chunks.into_inner().yellow()
        )
    };
    anstream::println!("{report}");

    Ok(())
}

fn collect_region_files(base_path: &Path) -> anyhow::Result<Vec<PathBuf>> {
    log::debug!("Collecting files.");
    let mut files = vec![];
    for sub_folder in REGION_SUBFOLDERS {
        let path = base_path.join(Path::new(sub_folder));
        log::debug!("Checking {:?} for region files.", path);
        if !path.try_exists().map_or(false, |b| b) {
            continue;
        }
        let mut contents = path
            .read_dir()?
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                if let Some(ext) = path.extension() {
                    ext == "mca"
                } else {
                    false
                }
            })
            .collect();
        files.append(&mut contents);
    }
    log::debug!("Collected {} files.", files.len());
    Ok(files)
}

fn process_region_file(region_file_path: &Path, man_inhabited_time: usize) -> anyhow::Result<u16> {
    let mut deleted_chunks = 0;
    let region_file = File::options()
        .read(true)
        .write(true)
        .open(region_file_path)?;
    let mut region = Region::from_stream(region_file)?;
    for x in 0..32 {
        for y in 0..32 {
            let Ok(Some(chunk))= region.read_chunk(x, y) else { continue; };
            let chunk: Chunk = fastnbt::from_bytes(&chunk)?;
            if chunk.inhabited_time <= (man_inhabited_time / 20) {
                deleted_chunks += 1;
                region.remove_chunk(x, y)?;
            }
        }
    }

    // truncate region file
    let mut region_file = region.into_inner()?;
    let len = region_file.stream_position()?;
    region_file.set_len(len)?;

    Ok(deleted_chunks)
}

// Thank you stackoverflow lol
fn dir_size(path: &Path) -> io::Result<u64> {
    fn dir_size(mut dir: fs::ReadDir) -> io::Result<u64> {
        dir.try_fold(0, |acc, file| {
            let file = file?;
            let size = match file.metadata()? {
                data if data.is_dir() => dir_size(fs::read_dir(file.path())?)?,
                data => data.len(),
            };
            Ok(acc + size)
        })
    }

    dir_size(fs::read_dir(path)?)
}
