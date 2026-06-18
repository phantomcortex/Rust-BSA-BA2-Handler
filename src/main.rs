//! BSA/BA2 Archive Tool
//!
//! A GUI + CLI application for packing and unpacking Bethesda BSA/BA2 archives
//! with support for multiple game formats.

mod archive;
mod gui;

use archive::{
    detect_game_version, extract_archive_files_batch, list_archive_files, unpack_archive_to,
    Ba2Builder, Ba2Format, BsaBuilder, GameVersion,
};
use gui::state::{setup_callbacks, AppState};
use gui::MainWindow;
use slint::ComponentHandle;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tracing_subscriber::EnvFilter;
use walkdir::WalkDir;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        return run_gui(None);
    }

    if args[1] == "--gui" {
        let preload = args.get(2).map(PathBuf::from);
        return run_gui(preload);
    }

    // CLI mode
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .init();

    let command = args[1].as_str();
    match command {
        "unpack" | "extract" => cli_unpack(&args[2..]),
        "extract-files" => cli_extract_files(&args[2..]),
        "pack" => cli_pack(&args[2..]),
        "add-files" => cli_add_files(&args[2..]),
        "list" | "ls" => cli_list(&args[2..]),
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        other => {
            eprintln!("Unknown command: {}", other);
            print_help();
            std::process::exit(1);
        }
    }
}

fn run_gui(preload: Option<PathBuf>) -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let window = MainWindow::new()?;
    let state = Arc::new(Mutex::new(AppState::new()));
    setup_callbacks(&window, state.clone());

    if let Some(path) = preload {
        let mut s = state.lock().unwrap();
        match s.load_archive(&path) {
            Ok(()) => {
                let title = format!(
                    "{} - BSA/BA2 Tool",
                    path.file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default()
                );
                let total = s.total_count();
                let selected = s.selected_count();
                let model = s.to_slint_model();
                drop(s);
                window.set_window_title(slint::SharedString::from(&title));
                window.set_pack_mode(false);
                window.set_tree_nodes(model);
                window.set_status_text(slint::SharedString::from(format!(
                    "{} files, {} selected",
                    total, selected
                )));
            }
            Err(e) => {
                drop(s);
                window.set_status_text(slint::SharedString::from(format!(
                    "Error loading archive: {}",
                    e
                )));
            }
        }
    }

    window.run()?;
    Ok(())
}

fn print_help() {
    eprintln!(
        "BSA/BA2 Archive Tool

USAGE:
    bsa-ba2-tool                                              Launch GUI
    bsa-ba2-tool --gui [archive]                              Launch GUI, optionally pre-loading an archive
    bsa-ba2-tool unpack <archive> [output]                    Extract all files to folder
    bsa-ba2-tool extract-files <archive> <output> [files...]  Extract specific files
    bsa-ba2-tool pack <folder> <output> <game>                Pack folder into archive (create)
    bsa-ba2-tool add-files <archive> <game|auto> <base> [files...]  Add/overwrite files (patch)
    bsa-ba2-tool list <archive>                               List files (outputs SIZE\\tPATH per line)

GAME VERSIONS:"
    );
    for v in GameVersion::all() {
        eprintln!("    {:<14} {}", v.cli_name(), v.display_name());
    }
    eprintln!(
        "
EXAMPLES:
    bsa-ba2-tool unpack Skyrim.bsa ./output
    bsa-ba2-tool pack ./my_mod my_mod.bsa skyrimse
    bsa-ba2-tool pack ./textures textures.ba2 fo4ng-v7
    bsa-ba2-tool list archive.ba2"
    );
}

fn cli_list(args: &[String]) -> anyhow::Result<()> {
    if args.is_empty() {
        eprintln!("Usage: bsa-ba2-tool list <archive>");
        std::process::exit(1);
    }

    let archive_path = Path::new(&args[0]);
    let files = list_archive_files(archive_path)?;

    for entry in &files {
        println!("{}\t{}", entry.size, entry.path);
    }
    if let Some(game) = detect_game_version(archive_path) {
        eprintln!("Game: {}", game.cli_name());
    }
    eprintln!("{} files", files.len());
    Ok(())
}

fn cli_unpack(args: &[String]) -> anyhow::Result<()> {
    if args.is_empty() {
        eprintln!("Usage: bsa-ba2-tool unpack <archive> [output_folder]");
        std::process::exit(1);
    }

    let archive_path = PathBuf::from(&args[0]);
    let output_folder = if args.len() > 1 {
        PathBuf::from(&args[1])
    } else {
        // Default: archive name without extension
        let stem = archive_path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "output".to_string());
        archive_path.parent().unwrap_or(Path::new(".")).join(stem)
    };

    let files = list_archive_files(&archive_path)?;
    let total = files.len();
    eprintln!("Extracting {} files from {}", total, archive_path.display());

    std::fs::create_dir_all(&output_folder)?;

    let file_paths: Vec<String> = files.iter().map(|e| e.path.clone()).collect();
    let extracted = std::sync::atomic::AtomicUsize::new(0);
    let idx = std::sync::atomic::AtomicUsize::new(0);

    extract_archive_files_batch(&archive_path, &file_paths, |path, data| {
        let out_path = output_folder.join(path.replace('\\', "/"));
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&out_path, &data)?;
        extracted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let current = idx.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;

        if current.is_multiple_of(500) || current == total {
            eprint!("\r  {}/{} files extracted", current, total);
        }
        Ok(())
    })?;

    eprintln!();
    eprintln!(
        "Done: {} of {} files extracted to {}",
        extracted.load(std::sync::atomic::Ordering::Relaxed),
        total,
        output_folder.display()
    );
    Ok(())
}

fn cli_extract_files(args: &[String]) -> anyhow::Result<()> {
    if args.len() < 2 {
        eprintln!("Usage: bsa-ba2-tool extract-files <archive> <output-dir> [file1 file2 ...]");
        std::process::exit(1);
    }

    let archive_path = PathBuf::from(&args[0]);
    let output_dir = PathBuf::from(&args[1]);
    let wanted_files: Vec<String> = args[2..].to_vec();

    std::fs::create_dir_all(&output_dir)?;

    if wanted_files.is_empty() {
        // No specific files listed — extract everything (same as unpack)
        let all_files = list_archive_files(&archive_path)?;
        let file_paths: Vec<String> = all_files.iter().map(|e| e.path.clone()).collect();
        let extracted = std::sync::atomic::AtomicUsize::new(0);
        extract_archive_files_batch(&archive_path, &file_paths, |path, data| {
            let out_path = output_dir.join(path.replace('\\', "/"));
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&out_path, &data)?;
            extracted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Ok(())
        })?;
        eprintln!("{} files extracted", extracted.load(std::sync::atomic::Ordering::Relaxed));
        return Ok(());
    }

    let extracted = std::sync::atomic::AtomicUsize::new(0);
    extract_archive_files_batch(&archive_path, &wanted_files, |path, data| {
        let out_path = output_dir.join(path.replace('\\', "/"));
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&out_path, &data)?;
        extracted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    })?;

    eprintln!("{} files extracted", extracted.load(std::sync::atomic::Ordering::Relaxed));
    Ok(())
}

/// Core pack logic: walk source_folder, build archive at output_path with game_version.
/// Returns the number of files packed.
fn pack_folder_impl(
    source_folder: &Path,
    output_path: &Path,
    game_version: GameVersion,
) -> anyhow::Result<usize> {
    let mut file_paths: Vec<String> = Vec::new();
    for entry in WalkDir::new(source_folder)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            if let Ok(rel) = entry.path().strip_prefix(source_folder) {
                file_paths.push(rel.to_string_lossy().to_string());
            }
        }
    }

    if file_paths.is_empty() {
        anyhow::bail!("No files found in {}", source_folder.display());
    }

    let total = file_paths.len();
    eprintln!(
        "Packing {} files as {} -> {}",
        total,
        game_version.display_name(),
        output_path.display()
    );

    if game_version.is_ba2() {
        let ba2_version = game_version.ba2_version().unwrap_or_default();
        let compression = game_version.ba2_compression();

        let name_lower = output_path
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let format = if name_lower.contains("textures") {
            Ba2Format::DX10
        } else {
            Ba2Format::General
        };

        let mut builder = Ba2Builder::new()
            .with_version(ba2_version)
            .with_compression(compression)
            .with_format(format);

        for (idx, rel_path) in file_paths.iter().enumerate() {
            let disk_path = source_folder.join(rel_path.replace('\\', "/"));
            let data = std::fs::read(&disk_path)?;
            builder.add_file(rel_path, data);

            if (idx + 1) % 100 == 0 || idx + 1 == total {
                eprint!("\r  Reading: {}/{}", idx + 1, total);
            }
        }
        eprintln!();

        eprintln!("  Building archive...");
        builder.build_with_progress(output_path, |current, btotal, _| {
            if current % 100 == 0 || current == btotal {
                eprint!("\r  Compressing: {}/{}", current, btotal);
            }
        })?;
        eprintln!();
    } else {
        let bsa_version = game_version.bsa_version().unwrap();
        let compress = game_version.supports_compression();

        let mut builder = BsaBuilder::new()
            .with_version(bsa_version)
            .with_compression(compress);

        for (idx, rel_path) in file_paths.iter().enumerate() {
            let disk_path = source_folder.join(rel_path.replace('\\', "/"));
            let data = std::fs::read(&disk_path)?;
            builder.add_file(rel_path, data);

            if (idx + 1) % 100 == 0 || idx + 1 == total {
                eprint!("\r  Reading: {}/{}", idx + 1, total);
            }
        }
        eprintln!();

        eprintln!("  Building archive...");
        builder.build_with_progress(output_path, |current, btotal, _| {
            if current % 100 == 0 || current == btotal {
                eprint!("\r  Compressing: {}/{}", current, btotal);
            }
        })?;
        eprintln!();
    }

    Ok(total)
}

fn cli_pack(args: &[String]) -> anyhow::Result<()> {
    if args.len() < 3 {
        eprintln!("Usage: bsa-ba2-tool pack <folder> <output> <game>");
        eprintln!("Run 'bsa-ba2-tool help' for game version list");
        std::process::exit(1);
    }

    let source_folder = PathBuf::from(&args[0]);
    let output_path = PathBuf::from(&args[1]);
    let game_version = match GameVersion::from_cli_name(&args[2]) {
        Some(v) => v,
        None => {
            eprintln!("Unknown game version: {}", args[2]);
            eprintln!("Valid options:");
            for v in GameVersion::all() {
                eprintln!("  {:<14} {}", v.cli_name(), v.display_name());
            }
            std::process::exit(1);
        }
    };

    if game_version.is_tes3() {
        anyhow::bail!("Morrowind TES3 BSA writing is not supported");
    }

    let total = pack_folder_impl(&source_folder, &output_path, game_version)?;
    eprintln!("Done: {} files packed into {}", total, output_path.display());
    Ok(())
}

fn cli_add_files(args: &[String]) -> anyhow::Result<()> {
    if args.len() < 3 {
        eprintln!("Usage: bsa-ba2-tool add-files <archive> <game|auto> <base-dir> [rel-file ...]");
        eprintln!("  game|auto  'auto' detects from existing archive; or specify a game name");
        eprintln!("  base-dir   directory from which rel-file paths are resolved");
        eprintln!("  rel-file   paths relative to base-dir to add or overwrite");
        eprintln!("Run 'bsa-ba2-tool help' for game version list");
        std::process::exit(1);
    }

    let archive_path = PathBuf::from(&args[0]);
    let game_str = &args[1];
    let base_dir = PathBuf::from(&args[2]);
    let new_files = &args[3..];

    let game_version = if game_str.eq_ignore_ascii_case("auto") {
        if !archive_path.exists() {
            anyhow::bail!(
                "Cannot auto-detect game version: '{}' does not exist. Specify a game name explicitly.",
                archive_path.display()
            );
        }
        detect_game_version(&archive_path).ok_or_else(|| {
            anyhow::anyhow!(
                "Could not detect game version from '{}'",
                archive_path.display()
            )
        })?
    } else {
        match GameVersion::from_cli_name(game_str) {
            Some(v) => v,
            None => {
                eprintln!("Unknown game version: {}", game_str);
                eprintln!("Valid options:");
                for v in GameVersion::all() {
                    eprintln!("  {:<14} {}", v.cli_name(), v.display_name());
                }
                std::process::exit(1);
            }
        }
    };

    if game_version.is_tes3() {
        anyhow::bail!("Morrowind TES3 BSA writing is not supported");
    }

    // Create temp staging directory
    let temp_dir = std::env::temp_dir().join(format!(
        "bsa-add-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos()
    ));
    std::fs::create_dir_all(&temp_dir)?;

    let result = (|| -> anyhow::Result<()> {
        // Unpack existing archive into staging if it exists
        if archive_path.exists() {
            eprintln!("Staging existing archive contents...");
            let n = unpack_archive_to(&archive_path, &temp_dir)?;
            eprintln!("  Staged {} existing files", n);
        }

        // Overlay new / modified files.
        // Each entry may be a regular file or a directory.  Directories are
        // walked recursively so that dragging a folder into file-roller adds
        // all of its contents at the correct relative paths.
        let mut overlaid = 0usize;
        for rel_path in new_files {
            let src = base_dir.join(rel_path.replace('\\', "/"));
            let dst = temp_dir.join(rel_path.replace('\\', "/"));

            let meta = std::fs::symlink_metadata(&src)
                .map_err(|e| anyhow::anyhow!("Cannot stat '{}': {}", src.display(), e))?;

            if meta.is_dir() {
                // Recursively copy all files under this directory
                for entry in WalkDir::new(&src).into_iter().filter_map(|e| e.ok()) {
                    if entry.file_type().is_file() {
                        let rel = entry.path().strip_prefix(&src).unwrap();
                        let dst_file = dst.join(rel);
                        if let Some(parent) = dst_file.parent() {
                            std::fs::create_dir_all(parent)?;
                        }
                        std::fs::copy(entry.path(), &dst_file)?;
                        overlaid += 1;
                    }
                }
            } else {
                // Regular file or symlink-to-file
                if let Some(parent) = dst.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::copy(&src, &dst)
                    .map(|_| ())
                    .map_err(|e| anyhow::anyhow!("Failed to stage '{}': {}", rel_path, e))?;
                overlaid += 1;
            }
        }
        if overlaid > 0 {
            eprintln!("  Overlaid {} new/modified files", overlaid);
        }

        // Repack staging dir → archive
        let total = pack_folder_impl(&temp_dir, &archive_path, game_version)?;
        eprintln!(
            "Done: {} files in '{}'",
            total,
            archive_path.display()
        );
        Ok(())
    })();

    // Always clean up the staging directory
    let _ = std::fs::remove_dir_all(&temp_dir);

    result
}
