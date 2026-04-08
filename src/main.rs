use std::{
    collections::HashSet,
    fs,
    io::{self, BufReader, BufWriter, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::Command,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Ok, Result, bail};
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use memmap2::Mmap;
use rayon::prelude::*;
use sha2::{Digest, Sha256};
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

    #[arg(
        short = 'd',
        long,
        help = "Use delta transfer for changed files (only changed chunks hitch a ride)"
    )]
    delta: bool,

    #[arg(
        short = 'z',
        long,
        help = "Compress file data during transfer (pack it in the kitty carrier)"
    )]
    compress: bool,

    #[arg(
        long,
        default_value_t = 3,
        value_parser = clap::value_parser!(i32).range(-7..=22),
        requires = "compress",
        help = "Compression level for --compress (-7 to 22, from kitten-soft to lion-tight)"
    )]
    compression_level: i32,
}

struct SyncStats {
    files_copied: usize,
    files_skipped: usize,
    files_deleted: usize,
    bytes_transferred: u64,
    bytes_on_wire: u64,
}

impl SyncStats {
    fn new() -> Self {
        Self {
            files_copied: 0,
            files_skipped: 0,
            files_deleted: 0,
            bytes_transferred: 0,
            bytes_on_wire: 0,
        }
    }

    fn merge(&mut self, other: &SyncStats) {
        self.files_copied += other.files_copied;
        self.files_skipped += other.files_skipped;
        self.files_deleted += other.files_deleted;
        self.bytes_transferred += other.bytes_transferred;
        self.bytes_on_wire += other.bytes_on_wire;
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
        if self.bytes_on_wire != self.bytes_transferred {
            let savings = self.bytes_transferred.saturating_sub(self.bytes_on_wire);
            let percent = if self.bytes_transferred == 0 {
                0.0
            } else {
                (savings as f64 / self.bytes_transferred as f64) * 100.0
            };
            println!(
                "  Packed bytes in kitty carrier: {} ({:.1}% saved)",
                format_bytes(self.bytes_on_wire),
                percent
            );
        }

        if self.files_copied == 0 && self.files_deleted == 0 {
            println!("\nEverything's already purr-fect! Time for a cat nap.");
        } else {
            println!("\nMission accomplished! *contented purring*");
        }
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    let source_raw = args.source.to_string_lossy().to_string();
    let destination_raw = args.destination.to_string_lossy().to_string();
    let source_remote = parse_remote_endpoint(&source_raw);
    let destination_remote = parse_remote_endpoint(&destination_raw);

    println!("CarSync - rsync with cars!");
    println!("==========================\n");

    if source_remote.is_some() && destination_remote.is_some() {
        bail!("Both source and destination are remote. At least one side must be local.")
    }

    if source_remote.is_some() {
        bail!("Remote source is not supported yet. Use local source -> remote destination for now.")
    }

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

    if let Some(remote) = destination_remote {
        println!("Remote destination detected - heading out over SSH...\n");
        sync_to_remote(&args, &remote, &mut stats)?;
    } else if args.source.is_file() {
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
            let (size, wire_bytes) = transfer_file(
                source,
                destination,
                args.delta,
                args.compress,
                args.compression_level,
            )?;
            stats.files_copied += 1;
            stats.bytes_transferred += size;
            stats.bytes_on_wire += wire_bytes;
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
                    let (size, wire_bytes) = transfer_file(
                        source_file,
                        &dest_file,
                        args.delta,
                        args.compress,
                        args.compression_level,
                    )?;
                    local_stats.files_copied += 1;
                    local_stats.bytes_transferred += size;
                    local_stats.bytes_on_wire += wire_bytes;
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

#[derive(Debug, Clone)]
struct RemoteEndpoint {
    host: String,
    path: String,
}

fn parse_remote_endpoint(raw: &str) -> Option<RemoteEndpoint> {
    let (host, path) = raw.split_once(':')?;
    if host.is_empty() || path.is_empty() || !host.contains('@') || host.contains('/') {
        return None;
    }
    Some(RemoteEndpoint {
        host: host.to_string(),
        path: path.to_string(),
    })
}

fn sync_to_remote(args: &Args, remote: &RemoteEndpoint, stats: &mut SyncStats) -> Result<()> {
    if args.delta {
        println!("Note: --delta is currently local-only, so remote sync uses whole-file transfer.");
    }

    if args.source.is_file() {
        sync_file_to_remote(args, &args.source, remote, &remote.path, stats)?;
    } else if args.source.is_dir() {
        if !args.recursive {
            bail!("That's a whole directory! Use -r / --recursive to crawl through it")
        }
        sync_directory_to_remote(args, remote, stats)?;
    }
    Ok(())
}

fn sync_file_to_remote(
    args: &Args,
    source: &Path,
    remote: &RemoteEndpoint,
    remote_destination: &str,
    stats: &mut SyncStats,
) -> Result<()> {
    if !args.dry_run {
        ensure_remote_parent_dir(remote, remote_destination)?;
    }

    let should_copy = if !remote_file_exists(remote, remote_destination)? {
        if args.verbose {
            println!("New remote file spotted: {} - pouncing!", source.display());
        }
        true
    } else if args.checksum {
        let matches = !remote_checksums_match(source, remote, remote_destination)?;
        if matches && args.verbose {
            println!("Sniffed difference in: {} - re-copying!", source.display());
        }
        matches
    } else {
        let matches = !remote_metadata_match(source, remote, remote_destination)?;
        if matches && args.verbose {
            println!("File changed: {} - updating!", source.display());
        }
        matches
    };

    if should_copy {
        if !args.dry_run {
            let size = fs::metadata(source)?.len();
            copy_file_to_remote(source, remote, remote_destination, args.compress)?;
            stats.files_copied += 1;
            stats.bytes_transferred += size;
            stats.bytes_on_wire += size;
        }
    } else {
        stats.files_skipped += 1;
    }

    Ok(())
}

fn sync_directory_to_remote(
    args: &Args,
    remote: &RemoteEndpoint,
    stats: &mut SyncStats,
) -> Result<()> {
    let source_files: Vec<PathBuf> = WalkDir::new(&args.source)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.path().to_path_buf())
        .collect();

    println!("Found {} files to inspect", source_files.len());

    if !args.dry_run {
        let mut dirs: HashSet<String> = HashSet::new();
        dirs.insert(remote.path.clone());
        for source_file in &source_files {
            let relative = source_file.strip_prefix(&args.source)?;
            if let Some(parent) = relative.parent() {
                dirs.insert(remote_join_path(&remote.path, parent));
            }
        }

        for dir in dirs {
            ensure_remote_dir(remote, &dir)?;
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

    for source_file in source_files {
        let relative_path = source_file.strip_prefix(&args.source)?;
        let remote_destination = remote_join_path(&remote.path, relative_path);
        let mut local_stats = SyncStats::new();

        let should_copy = if !remote_file_exists(remote, &remote_destination)? {
            true
        } else if args.checksum {
            !remote_checksums_match(&source_file, remote, &remote_destination)?
        } else {
            !remote_metadata_match(&source_file, remote, &remote_destination)?
        };

        if should_copy {
            if args.verbose {
                pb.println(format!("Pouncing on: {}", relative_path.display()));
            }
            if !args.dry_run {
                let size = fs::metadata(&source_file)?.len();
                copy_file_to_remote(&source_file, remote, &remote_destination, args.compress)?;
                local_stats.files_copied += 1;
                local_stats.bytes_transferred += size;
                local_stats.bytes_on_wire += size;
                pb.set_message("*carrying files in mouth*");
            }
        } else {
            local_stats.files_skipped += 1;
        }

        stats.merge(&local_stats);
        pb.inc(1);
    }

    pb.set_message("*licking paws*");
    pb.finish_with_message("Done prowling!");

    if args.delete {
        println!("\nLooking for files to knock off the remote table...");
        delete_extra_remote_files(args, remote, stats)?;
    }

    Ok(())
}

fn delete_extra_remote_files(
    args: &Args,
    remote: &RemoteEndpoint,
    stats: &mut SyncStats,
) -> Result<()> {
    let output = run_ssh(
        remote,
        &format!("find {} -type f", shell_quote(&remote.path)),
    )?;

    for line in output.lines() {
        let remote_file = line.trim();
        if remote_file.is_empty() {
            continue;
        }
        let Some(relative) = remote_relative_path(&remote.path, remote_file) else {
            continue;
        };
        let source_file = args.source.join(relative);
        if !source_file.exists() {
            if args.verbose || args.dry_run {
                println!("Knocking off remote table: {}", remote_file);
            }
            if !args.dry_run {
                run_ssh(remote, &format!("rm -f {}", shell_quote(remote_file)))?;
                stats.files_deleted += 1;
            }
        }
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

fn remote_file_exists(remote: &RemoteEndpoint, remote_path: &str) -> Result<bool> {
    let status = Command::new("ssh")
        .arg(&remote.host)
        .arg(format!("test -f {}", shell_quote(remote_path)))
        .status()
        .context("Failed to run ssh for remote file existence check")?;
    Ok(status.success())
}

fn remote_metadata_match(
    source: &Path,
    remote: &RemoteEndpoint,
    remote_path: &str,
) -> Result<bool> {
    let source_meta = fs::metadata(source)?;
    let output = run_ssh(
        remote,
        &format!("stat -c '%s %Y' {}", shell_quote(remote_path)),
    )?;
    let mut parts = output.trim().split_whitespace();
    let Some(size_str) = parts.next() else {
        return Ok(false);
    };
    let Some(modified_str) = parts.next() else {
        return Ok(false);
    };

    let remote_size: u64 = size_str
        .parse()
        .context("Invalid remote size from stat output")?;
    if source_meta.len() != remote_size {
        return Ok(false);
    }

    let remote_modified_secs: u64 = modified_str
        .parse()
        .context("Invalid remote modified time from stat output")?;
    let source_modified_secs = source_meta
        .modified()?
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    Ok(source_modified_secs <= remote_modified_secs)
}

fn remote_checksums_match(
    source: &Path,
    remote: &RemoteEndpoint,
    remote_path: &str,
) -> Result<bool> {
    let source_hash = calculate_checksum(source)?;
    let output = run_ssh(remote, &format!("sha256sum {}", shell_quote(remote_path)))?;
    let Some(remote_hex) = output.split_whitespace().next() else {
        return Ok(false);
    };
    let remote_hash = hex_to_bytes(remote_hex)?;
    Ok(source_hash == remote_hash)
}

fn copy_file_to_remote(
    source: &Path,
    remote: &RemoteEndpoint,
    remote_path: &str,
    compress_over_ssh: bool,
) -> Result<()> {
    let destination = format!("{}:{}", remote.host, shell_quote(remote_path));
    let mut cmd = Command::new("scp");
    if compress_over_ssh {
        cmd.arg("-C");
    }
    let status = cmd
        .arg(source)
        .arg(destination)
        .status()
        .context("Failed to run scp")?;

    if !status.success() {
        bail!("scp failed for {}", source.display());
    }
    Ok(())
}

fn ensure_remote_parent_dir(remote: &RemoteEndpoint, remote_path: &str) -> Result<()> {
    let parent = Path::new(remote_path)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| ".".to_string());
    ensure_remote_dir(remote, &parent)
}

fn ensure_remote_dir(remote: &RemoteEndpoint, remote_dir: &str) -> Result<()> {
    run_ssh(remote, &format!("mkdir -p {}", shell_quote(remote_dir))).map(|_| ())
}

fn remote_join_path(base: &str, relative: &Path) -> String {
    let rel = relative.to_string_lossy().replace('\\', "/");
    if rel.is_empty() {
        return base.to_string();
    }
    if base.ends_with('/') {
        format!("{base}{rel}")
    } else {
        format!("{base}/{rel}")
    }
}

fn remote_relative_path<'a>(base: &'a str, full: &'a str) -> Option<&'a str> {
    if let Some(stripped) = full.strip_prefix(base) {
        return Some(stripped.trim_start_matches('/'));
    }
    None
}

fn run_ssh(remote: &RemoteEndpoint, remote_command: &str) -> Result<String> {
    let output = Command::new("ssh")
        .arg(&remote.host)
        .arg(remote_command)
        .output()
        .context("Failed to run ssh")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("ssh command failed on {}: {}", remote.host, stderr.trim());
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn hex_to_bytes(hex: &str) -> Result<Vec<u8>> {
    if hex.len() % 2 != 0 {
        bail!("Invalid hex string length");
    }
    let mut out = Vec::with_capacity(hex.len() / 2);
    for i in (0..hex.len()).step_by(2) {
        let byte = u8::from_str_radix(&hex[i..i + 2], 16).context("Invalid hex digit")?;
        out.push(byte);
    }
    Ok(out)
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

fn transfer_file(
    source: &Path,
    destination: &Path,
    use_delta: bool,
    use_compression: bool,
    compression_level: i32,
) -> Result<(u64, u64)> {
    if use_delta && destination.exists() {
        transfer_with_delta(source, destination)
    } else if use_compression {
        transfer_with_compression(source, destination, compression_level)
    } else {
        fs::copy(source, destination)?;
        let size = fs::metadata(source)?.len();
        Ok((size, size))
    }
}

fn transfer_with_compression(
    source: &Path,
    destination: &Path,
    compression_level: i32,
) -> Result<(u64, u64)> {
    let source_meta = fs::metadata(source)?;
    let source_size = source_meta.len();

    let source_file =
        fs::File::open(source).with_context(|| format!("Failed to open source {:?}", source))?;
    let mut source_reader = BufReader::new(source_file);

    let mut encoder = zstd::stream::Encoder::new(Vec::new(), compression_level)
        .context("Failed to initialize compressor")?;
    io::copy(&mut source_reader, &mut encoder).context("Failed to compress file data")?;
    let compressed = encoder.finish().context("Failed to finalize compression")?;
    let compressed_size = compressed.len() as u64;

    let destination_file = fs::File::create(destination)
        .with_context(|| format!("Failed to create destination {:?}", destination))?;
    let mut destination_writer = BufWriter::new(destination_file);

    let mut decoder = zstd::stream::Decoder::new(compressed.as_slice())
        .context("Failed to initialize decoder")?;
    io::copy(&mut decoder, &mut destination_writer)
        .context("Failed to write decompressed content")?;
    destination_writer.flush()?;
    fs::set_permissions(destination, source_meta.permissions())?;

    Ok((source_size, compressed_size))
}

const DELTA_CHUNK_SIZE: usize = 64 * 1024;

fn transfer_with_delta(source: &Path, destination: &Path) -> Result<(u64, u64)> {
    let source_meta = fs::metadata(source)?;
    let source_size = source_meta.len();

    let signatures = build_destination_signatures(destination)?;
    let destination_file = fs::File::open(destination)
        .with_context(|| format!("Failed to open destination {:?}", destination))?;
    let mut destination_reader = BufReader::new(destination_file);

    let source_file =
        fs::File::open(source).with_context(|| format!("Failed to open source {:?}", source))?;
    let mut source_reader = BufReader::new(source_file);

    let tmp_path = temp_output_path(destination);
    let tmp_file = fs::File::create(&tmp_path)
        .with_context(|| format!("Failed to create temp output {:?}", tmp_path))?;
    let mut tmp_writer = BufWriter::new(tmp_file);

    let mut buffer = vec![0_u8; DELTA_CHUNK_SIZE];
    let mut bytes_on_wire = 0_u64;

    loop {
        let read_len = source_reader.read(&mut buffer)?;
        if read_len == 0 {
            break;
        }

        let chunk = &buffer[..read_len];
        let hash = sha256_chunk(chunk);

        if let Some(sig) = signatures
            .get(&hash)
            .and_then(|items| items.iter().find(|sig| sig.length == read_len))
        {
            destination_reader.seek(SeekFrom::Start(sig.offset))?;
            let mut match_buf = vec![0_u8; read_len];
            destination_reader.read_exact(&mut match_buf)?;
            tmp_writer.write_all(&match_buf)?;
        } else {
            tmp_writer.write_all(chunk)?;
            bytes_on_wire += read_len as u64;
        }
    }

    tmp_writer.flush()?;
    fs::rename(&tmp_path, destination)?;
    fs::set_permissions(destination, source_meta.permissions())?;

    Ok((source_size, bytes_on_wire))
}

#[derive(Debug)]
struct BlockSignature {
    offset: u64,
    length: usize,
}

fn build_destination_signatures(
    destination: &Path,
) -> Result<std::collections::HashMap<[u8; 32], Vec<BlockSignature>>> {
    let file = fs::File::open(destination)
        .with_context(|| format!("Failed to open destination {:?}", destination))?;
    let mut reader = BufReader::new(file);
    let mut signatures = std::collections::HashMap::<[u8; 32], Vec<BlockSignature>>::new();
    let mut offset = 0_u64;
    let mut buffer = vec![0_u8; DELTA_CHUNK_SIZE];

    loop {
        let read_len = reader.read(&mut buffer)?;
        if read_len == 0 {
            break;
        }

        let hash = sha256_chunk(&buffer[..read_len]);
        signatures.entry(hash).or_default().push(BlockSignature {
            offset,
            length: read_len,
        });
        offset += read_len as u64;
    }

    Ok(signatures)
}

fn sha256_chunk(chunk: &[u8]) -> [u8; 32] {
    let digest = Sha256::digest(chunk);
    let mut out = [0_u8; 32];
    out.copy_from_slice(&digest);
    out
}

fn temp_output_path(destination: &Path) -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let pid = std::process::id();
    destination.with_extension(format!("carsync.tmp.{pid}.{now}"))
}
