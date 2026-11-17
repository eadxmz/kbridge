use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

pub struct StrippedPackage {
    pub inf_path: PathBuf,
    pub orig_whql_cat: Option<PathBuf>,
}

const CORE_WDDM_FILES: &[&str] = &[
    "igdkmd64.sys",
    "igdumd64.dll",
    "igdumd32.dll",
    "ig4icd64.dll",
    "ig4icd32.dll",
    "igd10umd64.dll",
    "igd10umd32.dll",
    "igdde64.dll",
    "igdde32.dll",
    "igdumdx32.dll",
    "igfxress.dll",
];

const OPTIONAL_UI_FILES: &[&str] = &[
    "gfxui.exe",
    "gfxui.exe.config",
    "igfxrenu.lrc",
    "igfxcfg.dll",
    "igfxdev.dll",
    "igfxdevlib.dll",
    "hccutils.dll",
    "icclibdll_x64.dll",
    "igfxcpl.cpl",
];

fn find_display_inf(dir: &Path) -> Option<PathBuf> {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(found) = find_display_inf(&path) {
                    return Some(found);
                }
            } else if path.is_file() {
                let fname = path.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
                if fname.ends_with(".inf") && !fname.contains("intcdaud") && !fname.contains("autorun") {
                    if let Ok(text) = fs::read_to_string(&path) {
                        if text.contains("igdkmd64.sys") || text.contains("IntelGfx") {
                            return Some(path);
                        }
                    }
                }
            }
        }
    }
    None
}

pub fn strip_driver_package(
    src_dir: &Path,
    out_dir: &Path,
    keep_ui: bool,
) -> Result<StrippedPackage> {
    if out_dir.exists() {
        let _ = fs::remove_dir_all(out_dir);
    }
    fs::create_dir_all(out_dir)?;

    let src_inf = find_display_inf(src_dir).with_context(|| {
        format!(
            "Could not find an Intel Display .inf file (igdlh64.inf or kit*.inf) inside '{}'. Make sure you extracted the zip/exe first.",
            src_dir.display()
        )
    })?;

    // Only scan the directory containing the Display INF (e.g., `Graphics\`) so unrelated sub-packages aren't mixed in
    let graphics_dir = src_inf.parent().unwrap_or(src_dir);

    for entry in fs::read_dir(graphics_dir)?.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let fname = path.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
        let is_coinstaller = fname.starts_with("igfxcoin") && fname.ends_with(".dll");
        let is_microcode_blob = fname.ends_with(".bin") || fname.ends_with(".vp") || fname.ends_with(".cpa");

        let should_keep = CORE_WDDM_FILES.contains(&fname.as_str())
            || is_coinstaller
            || is_microcode_blob
            || (keep_ui && OPTIONAL_UI_FILES.contains(&fname.as_str()));

        if should_keep {
            fs::copy(&path, out_dir.join(path.file_name().unwrap()))?;
            println!("[+] Kept core file: {}", fname);
        }
    }

    // Match the exact WHQL catalog file with the same stem as the INF (e.g. kit51428.inf -> kit51428.cat)
    let inf_stem = src_inf
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    let mut found_cat: Option<PathBuf> = None;
    for entry in fs::read_dir(graphics_dir)?.flatten() {
        let path = entry.path();
        if path.is_file() {
            let fname = path.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
            if fname == format!("{}.cat", inf_stem) || fname == "igdlh.cat" {
                found_cat = Some(path);
                break;
            }
        }
    }

    let dst_inf = out_dir.join("igdlh64.inf");
    fs::copy(&src_inf, &dst_inf)?;
    println!(
        "[+] Imported display INF: {} -> igdlh64.inf",
        src_inf.file_name().unwrap().to_string_lossy()
    );

    if let Some(ref cat) = found_cat {
        println!(
            "[+] Matched original WHQL catalog: {}",
            cat.file_name().unwrap().to_string_lossy()
        );
    }

    Ok(StrippedPackage {
        inf_path: dst_inf,
        orig_whql_cat: found_cat,
    })
}
