use fastanvil::Region;
use rayon::prelude::{IntoParallelIterator, ParallelIterator};
use rayon::{ThreadPoolBuildError, ThreadPoolBuilder};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{self, Seek};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::mpsc;
use std::{fs, thread, time};

/// The subfolders in the world folder in which the region files are contained
const REGION_SUBFOLDERS: [&str; 3] = ["region", "DIM-1/region", "DIM1/region"];

pub struct Config {
    pub world_folder: PathBuf,
    pub max_inhabited_time: usize,
    pub thread_count: Option<usize>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub total_time_in_seconds: u64,
    pub total_freed_space_in_kib: u64,
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("The specified world folder could not be found")]
    WorldFolderNotFound,
    #[error("Unknown IO error")]
    IOError(#[from] io::Error),
    #[error("Failed to build Rayon threadpool")]
    RayonError(#[from] ThreadPoolBuildError),
}

pub enum ProcessingUpdate {
    Starting { total_files: u64 },
    ProcessedRegion(Result<ProcessedRegion, RegionProcessingError>),
    Finished(Report),
}

pub fn execute(config: Config) -> Result<mpsc::Receiver<ProcessingUpdate>, Error> {
    if !config.world_folder.try_exists().map_or(false, |r| r) {
        return Err(Error::WorldFolderNotFound);
    }

    if let Some(threads) = config.thread_count {
        ThreadPoolBuilder::new()
            .num_threads(threads)
            .build_global()?;
    }

    let (tx, rx) = mpsc::channel();

    let size_before = dir_size(config.world_folder.as_path())?;
    let start_time = time::Instant::now();

    let files = collect_region_files(Path::new(&config.world_folder))?;

    let running = AtomicBool::new(true);

    thread::spawn(move || {
        let _ = tx.send(ProcessingUpdate::Starting {
            total_files: files.len() as u64,
        });

        files.into_par_iter().for_each_with(tx.clone(), |t, path| {
            if !running.load(std::sync::atomic::Ordering::Relaxed) {
                return;
            }

            if let Err(_) = t.send(ProcessingUpdate::ProcessedRegion(process_region_file(
                path.as_path(),
                config.max_inhabited_time * 20,
            ))) {
                running.store(false, std::sync::atomic::Ordering::Relaxed)
            }
        });

        let freed_space = size_before - dir_size(config.world_folder.as_path()).unwrap_or(0);
        let time_taken = time::Instant::now() - start_time;

        tx.send(ProcessingUpdate::Finished(Report {
            total_time_in_seconds: time_taken.as_secs(),
            total_freed_space_in_kib: freed_space,
        }))
    });

    Ok(rx)
}

fn collect_region_files(base_path: &Path) -> io::Result<Vec<PathBuf>> {
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

#[derive(thiserror::Error, Debug)]
pub enum RegionProcessingError {
    #[error("Unknown IO error")]
    IOError(#[from] io::Error),
    #[error("Anvil error")]
    AnvilError(#[from] fastanvil::Error),
    #[error("NBT error")]
    NBTError(#[from] fastnbt::error::Error),
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Chunk {
    inhabited_time: usize,
}

pub struct ProcessedRegion {
    x: usize,
    y: usize,
    total_chunks: u16,
    deleted_chunks: u16,
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
        .map(|s| s.split(".").skip(1).collect::<Vec<_>>())
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
