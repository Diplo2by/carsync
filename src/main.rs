use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Ok, Result, bail};
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
}

fn main() -> Result<()> {
    let args = Args::parse();

    if args.dry_run {
        println!("DRY RUN MODE - No files will be modified")
    }
    sync_file(&args.source, &args.destination)?;

    Ok(())
}

fn sync_file(source: &Path, destination: &Path) -> Result<()> {
    fs::copy(source, destination)?;

    Ok(())
}
