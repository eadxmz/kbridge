use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};

const KEEP_FILES: &[&str] = &[
    "igdlh64.inf",
    "igdlh.cat",
    "igdkmd64.sys",
    "igdumd64.dll",
    "ig4icd64.dll",
    "igfxcoin_v2993.dll",
];

const OPTIONAL_UI_FILES: &[&str] = &[
    "gfxui.exe",
    "gfxui.exe.config",
    "igfxrenu.lrc",
    "igfxcfg.dll",
    "igfxdev.dll",
];

pub fn strip_driver_package(
    src_dir: &Path,
    out_dir: &Path,
    keep_ui: bool,
) -> Result<Option<PathBuf>> {
    fs::create_dir_all(out_dir)?;
    let graphics_src = if src_dir.join("Graphics").exists() {
        src_dir.join("Graphics")
    } else {
        src_dir.to_path_buf()
    };

    let mut orig_cat_path: Option<PathBuf> = None;

    for entry in fs::read_dir(&graphics_src)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            let fname = path.file_name().unwrap().to_string_lossy().to_lowercase();
            if fname == "igdlh.cat" {
                orig_cat_path = Some(path.clone());
            }
            let should_keep = KEEP_FILES.contains(&fname.as_str())
                || (keep_ui && OPTIONAL_UI_FILES.contains(&fname.as_str()));

            if should_keep {
                fs::copy(&path, out_dir.join(entry.file_name()))?;
                println!("[+] Kept core file: {}", fname);
            }
        }
    }
    Ok(orig_cat_path)
}
