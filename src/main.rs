use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Ok, Result, bail};
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use sha2::{Digest, Sha256};
use std::io::Read;
use walkdir::WalkDir;

#[derive(Parser, Debug)]
#[command(name = "carsync")]
#[command(about = "Rsync with Cars 🐱", long_about = None)]
struct Args {
    source: PathBuf,

    destination: PathBuf,

    #[arg(short = 'n', long)]
    dry_run: bool,

    #[arg(short, long)]
    verbose: bool,

    #[arg(short, long)]
    recursive: bool,

    #[arg(long)]
    delete: bool,

    #[arg(short, long)]
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

    fn print_summary(&self) {
        println!("\n🐱 Sync Summary:");
        println!("  Files copied: {}", self.files_copied);
        println!("  Files skipped: {}", self.files_skipped);
        println!("  Files deleted: {}", self.files_deleted);
        println!(
            "  Bytes transferred: {}",
            format_bytes(self.bytes_transferred)
        );
    }
}

fn create_cat_progress_bar(total: u64, style: &str) -> ProgressBar {
    let pb = ProgressBar::new(total);

    match style {
        "running" => {
            pb.set_style(
                ProgressStyle::default_bar()
                    .template(
                        "{spinner} [{elapsed_precise}] {bar:40.cyan/blue} {pos}/{len}\n  {msg}",
                    )
                    .unwrap()
                    .progress_chars("🐱💨>-")
                    .tick_strings(&[
                        "  /\\_/\\   ",
                        " ( o.o ) ~>",
                        "  > ^ <   ",
                        "    /\\_/\\  ",
                        "   ( ^.^ )~>",
                        "    > ^ <  ",
                    ]),
            );
        }
        "chasing" => {
            pb.set_style(
                ProgressStyle::default_bar()
                    .template(
                        "{spinner} [{elapsed_precise}] {bar:40.cyan/blue} {pos}/{len}\n  {msg}",
                    )
                    .unwrap()
                    .progress_chars("🐱🐁>-")
                    .tick_strings(&[
                        "🐱        🐁",
                        " 🐱       🐁",
                        "  🐱      🐁",
                        "   🐱     🐁",
                        "    🐱    🐁",
                        "     🐱   🐁",
                        "      🐱  🐁",
                        "       🐱 🐁",
                        "        🐱🐁",
                    ]),
            );
        }
        _ => {
            pb.set_style(
                ProgressStyle::default_bar()
                    .template("{spinner} [{elapsed_precise}] {bar:40.cyan/blue} {pos}/{len} {msg}")
                    .unwrap()
                    .progress_chars("😺😸😻")
                    .tick_strings(&["🐱", "😺", "😸", "😻", "😽"]),
            );
        }
    }

    pb.enable_steady_tick(std::time::Duration::from_millis(100));
    pb
}

fn get_cat_frames() -> Vec<&'static str> {
    vec![
        r#"
   /\_/\  
  ( o.o ) ~>
   > ^ 
  /|   |\
 (_|   |_)
"#,
        r#"
   /\_/\  
  ( ^.^ ) ~>
   > ^ 
   |   |
  /|   |\
"#,
        r#"
   /\_/\  
  ( o.o ) ~>
   > ^ 
  /     \
 /_|   |_\
"#,
        r#"
   /\_/\  
  ( ^.^ ) ~>
   > ^ 
   |   |
  _|   |_
"#,
    ]
}
fn main() -> Result<()> {
    let args = Args::parse();

    if !args.source.exists() {
        bail!("Invalid Source path: {:?}", args.source);
    }

    if args.dry_run {
        println!("DRY RUN MODE - No files will be modified")
    }
    let mut stats = SyncStats::new();

    if args.source.is_file() {
        sync_file(&args, &args.source, &args.destination, &mut stats)?;
    } else if args.source.is_dir() {
        if !args.recursive {
            bail!("Source is a directory. Use -r / --recursive to sync directories")
        }
        sync_directory(&args, &mut stats)?;
    }

    stats.print_summary();

    Ok(())
}

fn sync_file(args: &Args, source: &Path, destination: &Path, stats: &mut SyncStats) -> Result<()> {
    if let Some(parent) = destination.parent() {
        if !args.dry_run && !parent.exists() {
            fs::create_dir_all(parent)?;
        }
    }

    let should_copy = if !destination.exists() {
        true
    } else if args.checksum {
        !checksums_match(source, destination)?
    } else {
        !metadata_match(source, destination)?
    };

    if should_copy && !args.dry_run {
        fs::copy(source, destination)?;
        let size = fs::metadata(source)?.len();
        stats.files_copied += 1;
        stats.bytes_transferred += size;
    } else {
        stats.files_deleted += 1;
    }

    Ok(())
}

fn sync_directory(args: &Args, stats: &mut SyncStats) -> Result<()> {
    if !args.dry_run && !args.destination.exists() {
        fs::create_dir_all(&args.destination).context("Failed to create destination directory")?;
    }

    let source_files: Vec<PathBuf> = WalkDir::new(&args.source)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.path().to_path_buf())
        .collect();

    let pb = if !args.verbose {
        let pb = ProgressBar::new(source_files.len() as u64);

        // Cat-themed progress bar styles
        let cat_chars = get_cat_progress_chars("running");

        // Option 1: Simple emoji cat running
        let style = ProgressStyle::default_bar()
            .template(&format!("{{spinner}} [{{elapsed_precise}}] {{bar:40.cyan/blue}} {{pos}}/{{len}} {{msg}}\n  🐱 {{wide_msg}}"))
            .unwrap()
            .progress_chars(cat_chars)
            .tick_strings(&["🐱", "😺", "😸", "😻", "😽", "🐈"]);

        pb.set_style(style);
        pb.enable_steady_tick(std::time::Duration::from_millis(120));
        Some(pb)
    } else {
        None
    };

    for source_file in &source_files {
        let relative_path = source_file.strip_prefix(&args.source)?;
        let dest_file = args.destination.join(relative_path);

        if let Some(ref pb) = pb {
            pb.set_message(format!("{}", relative_path.display()))
        }

        sync_file(args, source_file, &dest_file, stats)?;
        if let Some(ref pb) = pb {
            pb.inc(1);
        }
    }

    if let Some(pb) = pb {
        pb.finish_with_message("Done");
    }

    if args.delete && args.destination.exists() {
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
                println!("Deleting: {}", dest_file.display());
            }

            if !args.dry_run {
                fs::remove_file(&dest_file)?;
                println!("removed file {:?}", dest_file);
                stats.files_deleted += 1;
            }
        }
    }

    Ok(())
}

fn metadata_match(source: &Path, destination: &Path) -> Result<bool> {
    let source_meta = fs::metadata(source)?;
    let dest_meta = fs::metadata(destination)?;

    Ok(source_meta.len() == dest_meta.len() && source_meta.modified()? <= dest_meta.modified()?)
}

fn checksums_match(source: &Path, destination: &Path) -> Result<bool> {
    let source_hash = calculate_checksum(source)?;
    let dest_hash = calculate_checksum(destination)?;
    Ok(source_hash == dest_hash)
}

fn calculate_checksum(path: &Path) -> Result<Vec<u8>> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0; 8192];

    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    Ok(hasher.finalize().to_vec())
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
fn get_cat_progress_chars(style: &str) -> &'static str {
    match style {
        "running" => "🐱💨",
        "walking" => "🐱🐾",
        "sliding" => "😺💨",
        "chasing" => "🐱🐁",
        _ => "🐱💨",
    }
}
