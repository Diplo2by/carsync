use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Ok, Result, bail};
use clap::Parser;
use sha2::{Digest, Sha256};
use std::io::Read;
use walkdir::WalkDir;

#[derive(Parser, Debug)]
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


fn main() -> Result<()> {
    let args = Args::parse();

    if !args.source.exists() {
        bail!("Invalid Source path: {:?}", args.source);
    }

    if args.dry_run {
        println!("DRY RUN MODE - No files will be modified")
    }
    if args.source.is_file() {
        sync_file(&args, &args.source, &args.destination)?;
    } else if args.source.is_dir() {
        if !args.recursive {
            bail!("Source is a directory. Use -r / --recursive to sync directories")
        }
        sync_directory(&args)?;
    }

    Ok(())
}

fn sync_file(args: &Args, source: &Path, destination: &Path) -> Result<()> {
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
    }

    Ok(())
}

fn sync_directory(args: &Args) -> Result<()> {
    if !args.dry_run && !args.destination.exists() {
        fs::create_dir_all(&args.destination).context("Failed to create destination directory")?;
    }

    let source_files: Vec<PathBuf> = WalkDir::new(&args.source)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.path().to_path_buf())
        .collect();

    for source_file in &source_files {
        let relative_path = source_file.strip_prefix(&args.source)?;
        let dest_file = args.destination.join(relative_path);

        sync_file(args, source_file, &dest_file)?;
    }
    if args.delete && args.destination.exists() {
        delete_extra_files(args)?;
    }

    Ok(())
}

fn delete_extra_files(args: &Args) -> Result<()> {
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
