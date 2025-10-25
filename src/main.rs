use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Ok, Result, bail};
use clap::Parser;
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
}

fn main() -> Result<()> {
    let args = Args::parse();

    if args.dry_run {
        println!("DRY RUN MODE - No files will be modified")
    }
    if args.source.is_file() {
        sync_file(&args.source, &args.destination)?;
    } else if args.source.is_dir() {
        if !args.recursive {
            bail!("Source is a directory. Use -r / --recursive to sync directories")
        }
        sync_directory(&args)?;
    }

    Ok(())
}

fn sync_file(source: &Path, destination: &Path) -> Result<()> {
    fs::copy(source, destination)?;

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

        sync_file(source_file, &dest_file)?;
    }

    Ok(())
}
