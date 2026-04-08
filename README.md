# CarSync

CarSync - rsync with cars!

## Features

- Fast parallel file synchronization
- Checksum-based comparison for accurate syncing
- Optional deletion of extra files
- Dry-run mode to preview changes
- Progress bars with cat-themed messages
- Memory-mapped I/O for large files
- Optional compression for transfer-heavy workloads

## Installation

### From crates.io

```bash
cargo install carsync
```

### From source

```bash
git clone https://github.com/Diplo2by/carsync
cd carsync
cargo install --path .
```

## Usage

Basic recursive sync:

```bash
carsync -r /source/path /destination/path
```

### Options

- `-r, --recursive` - Crawl through subdirectories recursively
- `-n, --dry-run` - Just stalk, don't pounce (preview mode)
- `-v, --verbose` - Meow loudly about what's happening
- `-c, --checksum` - Compare files by checksum (sniff carefully)
- `-z, --compress` - Compress file data during transfer
- `--compression-level <LEVEL>` - Compression level for `--compress` (`-7` to `22`, default `3`)
- `--delete` - Delete files that don't belong (knock them off the table)

### Examples

Sync directories with verbose output:

```bash
carsync -r -v ~/Documents ~/Backup/Documents
```

Preview what would be synced (dry run):

```bash
carsync -r -n ~/Photos ~/Backup/Photos
```

Sync with checksum verification:

```bash
carsync -r -c ~/Projects ~/Backup/Projects
```

Sync and delete extra files in destination:

```bash
carsync -r --delete ~/Music ~/Backup/Music
```

Sync with compression enabled:

```bash
carsync -r -z ~/Data ~/Backup/Data
```

Sync with a custom compression level:

```bash
carsync -r -z --compression-level 8 ~/Data ~/Backup/Data
```

## License

This project is licensed under the GNU General Public License v3.0 - see the [LICENSE](LICENSE) file for details.
