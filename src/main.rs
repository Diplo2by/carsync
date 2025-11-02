use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

use anyhow::{Context, Ok, Result, bail};
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use memmap2::Mmap;
use rayon::prelude::*;
use sha2::{Digest, Sha256};
use std::io::Read;
use walkdir::WalkDir;

#[derive(Parser, Debug)]
#[command(name = "carsync")]
#[command(about = "CarSync - rsync with cars!", long_about = None)]
struct Args {
    source: PathBuf,

    destination: PathBuf,

    #[arg(short = 'n', long, help = "Dry run - just stalk, don't pounce")]
    dry_run: bool,

    #[arg(short, long, help = "Meow loudly about what's happening")]
    verbose: bool,

    #[arg(short, long, help = "Crawl through subdirectories recursively")]
    recursive: bool,

    #[arg(
        long,
        help = "Delete files that don't belong (knock them off the table)"
    )]
    delete: bool,

    #[arg(short, long, help = "Compare files by checksum (sniff carefully)")]
    checksum: bool,
}

struct SyncStats {
    files_copied: usize,
    files_skipped: usize,
    files_deleted: usize,
    bytes_transferred: u64,
}

impl SyncStats {
    fn new() -> Self {
        Self {
            files_copied: 0,
            files_skipped: 0,
            files_deleted: 0,
            bytes_transferred: 0,
        }
    }

    fn merge(&mut self, other: &SyncStats) {
        self.files_copied += other.files_copied;
        self.files_skipped += other.files_skipped;
        self.files_deleted += other.files_deleted;
        self.bytes_transferred += other.bytes_transferred;
    }

    fn print_summary(&self) {
        println!("\nPurr-fect Sync Summary:");
        println!("  Files pounced on (copied): {}", self.files_copied);
        println!("  Files left alone (skipped): {}", self.files_skipped);
        println!("  Files knocked off (deleted): {}", self.files_deleted);
        println!(
            "  Data 🐟 carried in kitty's mouth: {}",
            format_bytes(self.bytes_transferred)
        );

        if self.files_copied == 0 && self.files_deleted == 0 {
            println!("\nEverything's already purr-fect! Time for a cat nap.");
        } else {
            println!("\nMission accomplished! *contented purring*");
        }
    }
}

fn main() -> Result<()> {
    let args = Args::parse();

    println!("CarSync - rsync with cars!");
    println!("==========================\n");

    if !args.source.exists() {
        bail!(
            "Cat can't find source: {:?}\n   (Did it hide under the couch?)",
            args.source
        );
    }

    if args.dry_run {
        println!("DRY RUN MODE - Just watching, not pouncing")
    }
    let mut stats = SyncStats::new();

    if args.source.is_file() {
        println!("Single file detected - preparing to pounce...\n");
        sync_file(&args, &args.source, &args.destination, &mut stats)?;
    } else if args.source.is_dir() {
        if !args.recursive {
            bail!("That's a whole directory! Use -r / --recursive to crawl through it")
        }
        println!("Directory detected - time to explore every nook and cranny 😼...\n");
        sync_directory(&args, &mut stats)?;
    }

    stats.print_summary();

    Ok(())
}

fn sync_file(args: &Args, source: &Path, destination: &Path, stats: &mut SyncStats) -> Result<()> {
    if let Some(parent) = destination.parent() {
        if !args.dry_run && !parent.exists() {
            if args.verbose {
                println!("Creating cozy spot for file: {:?}", parent);
            }
            fs::create_dir_all(parent)?;
        }
    }

    let should_copy = if !destination.exists() {
        if args.verbose {
            println!("New file spotted: {} - pouncing!", source.display());
        }
        true
    } else if args.checksum {
        let matches = !checksums_match(source, destination)?;
        if matches && args.verbose {
            println!("Sniffed difference in: {} - re-copying!", source.display());
        }
        matches
    } else {
        let matches = !metadata_match(source, destination)?;
        if matches && args.verbose {
            println!("File changed: {} - updating!", source.display());
        }
        matches
    };

    if should_copy {
        if !args.dry_run {
            fs::copy(source, destination)?;
            let size = fs::metadata(source)?.len();
            stats.files_copied += 1;
            stats.bytes_transferred += size;
        }
    } else {
        if args.verbose {
            println!("Already purr-fect: {}", source.display());
        }
        stats.files_skipped += 1;
    }

    Ok(())
}

fn sync_directory(args: &Args, stats: &mut SyncStats) -> Result<()> {
    if !args.dry_run && !args.destination.exists() {
        println!("Building new cat tower at destination...\n");
        fs::create_dir_all(&args.destination).context("Failed to create destination directory")?;
    }

    let source_files: Vec<PathBuf> = WalkDir::new(&args.source)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.path().to_path_buf())
        .collect();

    println!("Found {} files to inspect", source_files.len());

    if !args.dry_run {
        let dirs: std::collections::HashSet<PathBuf> = source_files
            .iter()
            .filter_map(|f| {
                f.strip_prefix(&args.source)
                    .ok()
                    .and_then(|rel| args.destination.join(rel).parent().map(|p| p.to_path_buf()))
            })
            .collect();

        for dir in dirs {
            fs::create_dir_all(dir)?;
        }
    }

    let pb = ProgressBar::new(source_files.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("[{bar:40.cyan/blue}] {pos}/{len} files | {msg}")
            .unwrap()
            .progress_chars("=> "),
    );
    pb.set_message("*stalking files*");

    let stats_mutex = Mutex::new(SyncStats::new());

    source_files
        .par_iter()
        .try_for_each(|source_file| -> Result<()> {
            let relative_path = source_file.strip_prefix(&args.source)?;
            let dest_file = args.destination.join(relative_path);

            let mut local_stats = SyncStats::new();

            let should_copy = if !dest_file.exists() {
                true
            } else if args.checksum {
                !checksums_match(source_file, &dest_file)?
            } else {
                !metadata_match(source_file, &dest_file)?
            };

            if should_copy {
                if args.verbose {
                    pb.println(format!("Pouncing on: {}", relative_path.display()));
                }
                if !args.dry_run {
                    fs::copy(source_file, &dest_file)?;
                    let size = fs::metadata(source_file)?.len();
                    local_stats.files_copied += 1;
                    local_stats.bytes_transferred += size;
                    pb.set_message("*carrying files in mouth*");
                }
            } else {
                local_stats.files_skipped += 1;
            }

            let mut stats = stats_mutex.lock().unwrap();
            stats.merge(&local_stats);

            pb.inc(1);
            Ok(())
        })?;

    pb.set_message("*licking paws*");
    pb.finish_with_message("Done prowling!");

    *stats = stats_mutex.into_inner().unwrap();

    if args.delete && args.destination.exists() {
        println!("\nLooking for files to knock off the table...");
        delete_extra_files(args, stats)?;
    }

    Ok(())
}

fn delete_extra_files(args: &Args, stats: &mut SyncStats) -> Result<()> {
    let dest_files: Vec<PathBuf> = WalkDir::new(&args.destination)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.path().to_path_buf())
        .collect();

    for dest_file in dest_files {
        let relative_path = dest_file.strip_prefix(&args.destination)?;
        let source_file = args.source.join(relative_path);

        if !source_file.exists() {
            if args.verbose || args.dry_run {
                println!("Knocking off table: {}", dest_file.display());
            }

            if !args.dry_run {
                fs::remove_file(&dest_file)?;
                stats.files_deleted += 1;
            }
        }
    }

    Ok(())
}

fn metadata_match(source: &Path, destination: &Path) -> Result<bool> {
    let source_meta = fs::metadata(source)?;
    let dest_meta = fs::metadata(destination)?;

    if source_meta.len() != dest_meta.len() {
        return Ok(false);
    }

    Ok(source_meta.modified()? <= dest_meta.modified()?)
}

fn checksums_match(source: &Path, destination: &Path) -> Result<bool> {
    let source_hash = calculate_checksum(source)?;
    let dest_hash = calculate_checksum(destination)?;
    Ok(source_hash == dest_hash)
}

fn calculate_checksum(path: &Path) -> Result<Vec<u8>> {
    let file = fs::File::open(path)?;
    let metadata = file.metadata()?;

    if metadata.len() > 1_048_576 {
        let mmap = unsafe { Mmap::map(&file)? };
        let mut hasher = Sha256::new();
        hasher.update(&mmap);
        Ok(hasher.finalize().to_vec())
    } else {
        let mut hasher = Sha256::new();
        let mut buffer = [0; 65536];
        let mut file = file;

        loop {
            let bytes_read = file.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }
            hasher.update(&buffer[..bytes_read]);
        }
        Ok(hasher.finalize().to_vec())
    }
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit_index = 0;

    while size >= 1024.0 && unit_index < UNITS.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }

    format!("{:.2} {}", size, UNITS[unit_index])
}
