//! See [`execute`] for the entrypoint of this crate.

use fastanvil::Region;
use rayon::prelude::{IntoParallelIterator, ParallelIterator};
use rayon::{ThreadPoolBuildError, ThreadPoolBuilder};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{self, Seek};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::sync::mpsc;
use std::time::Duration;
use std::{fs, thread, time};

/// The subfolders in the world folder in which the region files are contained
const REGION_SUBFOLDERS: [&str; 3] = ["region", "DIM-1/region", "DIM1/region"];

/// The config to be passed to lessanvil.
#[derive(Default)]
pub struct Config {
    /// The folder containing the world.
    pub world_folder: PathBuf,
    /// The maximum [Inhabited Time](https://minecraft.fandom.com/wiki/Chunk_format) value for a chunk to get deleted.
    pub max_inhabited_time: usize,
    /// The amount of threads lessanvil should use.
    pub thread_count: usize,
}

/// A Report that will be handed out ofter the execution finished.
#[derive(Serialize)]
pub struct Report {
    /// The total time the execution took.
    pub time_taken: Duration,
    /// The total disk space freed in bytes.
    pub total_freed_space: u64,
    /// The total amount of region(-file-)s processed.
    pub total_regions: u64,
    /// The total amount of chunks processed.
    pub total_chunks: u64,
    /// The total amount of deleted chunks.
    pub total_deleted_chunks: u64,
}

/// The error type for errors that occured before the actual processing started.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// The world folder could not be accessed. This can be caused by e.g. the world folder not existing or the user not having sufficient privileges.
    #[error("The specified world folder could not be found")]
    WorldFolderNotFound,
    /// An arbitrary IO error.
    #[error("Unknown IO error")]
    IOError(#[from] io::Error),
    /// An error caused when invoking the [`ThreadPoolBuilder`]
    #[error("Failed to build Rayon threadpool")]
    RayonError(#[from] ThreadPoolBuildError),
}

/// An update during lessanvil's execution.
pub enum ProcessingUpdate {
    /// Only sent once after the processing started.
    Starting { total_files: u64 },
    /// Sent after a region has been processed.
    /// Contains the [`Result`] of the processed region.
    ProcessedRegion(Result<ProcessedRegion, RegionProcessingError>),
    /// Only sent once after the entire execution finished. This is the last message sent through the Channel.
    Finished(Report),
}

/// The entrypoint to this crate.
///
/// The [`Result`] contains a [`Receiver`](`mpsc::Receiver`) through which [`ProcessingUpdate`]s will be sent. Dropping this [`Receiver`](`mpsc::Receiver`) will stop the processing as soon as possible.
pub fn execute(config: Config) -> Result<mpsc::Receiver<ProcessingUpdate>, Error> {
    if !config.world_folder.try_exists().map_or(false, |r| r) {
        return Err(Error::WorldFolderNotFound);
    }

    ThreadPoolBuilder::new()
        .num_threads(config.thread_count)
        .build_global()?;

    let (tx, rx) = mpsc::channel();

    let files = collect_region_files(Path::new(&config.world_folder))?;

    let size_before = dir_size(config.world_folder.as_path())?;
    let start_time = time::Instant::now();
    let total_regions = files.len() as u64;
    let total_chunks = AtomicU64::new(0);
    let total_deleted_chunks = AtomicU64::new(0);

    thread::spawn(move || {
        let _ = tx.send(ProcessingUpdate::Starting {
            total_files: files.len() as u64,
        });

        let result = files
            .into_par_iter()
            .try_for_each_with(tx.clone(), |t, path| {
                let processed_region =
                    process_region_file(path.as_path(), config.max_inhabited_time * 20);

                if let Ok(ProcessedRegion {
                    x: _,
                    y: _,
                    total_chunks: chunks,
                    deleted_chunks,
                }) = processed_region
                {
                    total_chunks.fetch_add(chunks as u64, std::sync::atomic::Ordering::Relaxed);
                    total_deleted_chunks
                        .fetch_add(deleted_chunks as u64, std::sync::atomic::Ordering::Relaxed);
                }

                if t.send(ProcessingUpdate::ProcessedRegion(processed_region))
                    .is_err()
                {
                    Err(())
                } else {
                    Ok(())
                }
            });
        if result.is_ok() {
            let freed_space = size_before - dir_size(config.world_folder.as_path()).unwrap_or(0);
            let time_taken = time::Instant::now() - start_time;

            let _ = tx.send(ProcessingUpdate::Finished(Report {
                time_taken,
                total_freed_space: freed_space,
                total_regions,
                total_chunks: total_chunks.into_inner(),
                total_deleted_chunks: total_deleted_chunks.into_inner(),
            }));
        }
    });

    Ok(rx)
}

fn collect_region_files(base_path: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = vec![];
    for sub_folder in REGION_SUBFOLDERS {
        let path = base_path.join(Path::new(sub_folder));
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
    Ok(files)
}

/// The error type for processed regions.
#[derive(thiserror::Error, Debug)]
pub enum RegionProcessingError {
    /// An arbitrary I/0 Error
    #[error("Unknown I/O error")]
    IOError(#[from] io::Error),
    /// An arbitrary error for [Minecraft Anvil](https://minecraft.fandom.com/wiki/Anvil_file_format) operations.
    #[error("Anvil error")]
    AnvilError(#[from] fastanvil::Error),
    /// An arbitrary error for [Minecraft NBT](https://minecraft.fandom.com/wiki/NBT_format) operations.
    #[error("NBT error")]
    NBTError(#[from] fastnbt::error::Error),
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Chunk {
    inhabited_time: usize,
}

/// A processed region.
pub struct ProcessedRegion {
    /// The x-coordinate.
    pub x: usize,
    /// The y-coordinate.
    pub y: usize,
    /// The total chunks processed in this region.
    pub total_chunks: u16,
    /// The total chunks deleted in this region.
    pub deleted_chunks: u16,
}

fn process_region_file(
    region_file_path: &Path,
    man_inhabited_time: usize,
) -> Result<ProcessedRegion, RegionProcessingError> {
    let mut total_chunks = 0;
    let mut deleted_chunks = 0;

    let (y, x) = match region_file_path
        .file_stem()
        .and_then(|os| os.to_str())
        .map(|s| s.split('.').skip(1).collect::<Vec<_>>())
    {
        Some(mut vec) => (
            vec.pop().unwrap_or("0").parse::<usize>().unwrap_or(0),
            vec.pop().unwrap_or("0").parse::<usize>().unwrap_or(0),
        ),
        None => (0, 0),
    };

    let region_file = File::options()
        .read(true)
        .write(true)
        .open(region_file_path)?;
    let mut region = Region::from_stream(region_file)?;

    for x in 0..32 {
        for y in 0..32 {
            let Ok(Some(chunk)) = region.read_chunk(x, y) else { continue; };
            let chunk: Chunk = fastnbt::from_bytes(&chunk)?;
            total_chunks += 1;
            if chunk.inhabited_time <= (man_inhabited_time / 20) {
                region.remove_chunk(x, y)?;
                deleted_chunks += 1;
            }
        }
    }

    // truncate region file
    let mut region_file = region.into_inner()?;
    let len = region_file.stream_position()?;
    region_file.set_len(len)?;

    Ok(ProcessedRegion {
        x,
        y,
        total_chunks,
        deleted_chunks,
    })
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
