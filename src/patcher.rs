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
    "igcompkrng500.bin",
    "igcompkrng575.bin",
    "igcompkrng600.bin",
    "igfcg500m.bin",
    "igfcg575m.bin",
    "igfcg600m.bin",
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

fn collect_files_recursive(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let dname = path.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
                if dname.contains("audio") || dname.contains("hdmi") || dname.contains("opencl") || dname == "lang" {
                    continue;
                }
                collect_files_recursive(&path, out);
            } else if path.is_file() {
                out.push(path);
            }
        }
    }
}

pub fn strip_driver_package(
    src_dir: &Path,
    out_dir: &Path,
    keep_ui: bool,
) -> Result<StrippedPackage> {
    fs::create_dir_all(out_dir)?;

    let mut all_files = Vec::new();
    collect_files_recursive(src_dir, &mut all_files);

    let mut found_inf: Option<PathBuf> = None;
    let mut found_cat: Option<PathBuf> = None;

    for path in &all_files {
        let fname = path.file_name().unwrap_or_default().to_string_lossy().to_lowercase();

        if fname.ends_with(".inf") && !fname.contains("intcdaud") && !fname.contains("autorun") {
            if let Ok(text) = fs::read_to_string(path) {
                if text.contains("igdkmd64.sys") || text.contains("IntelGfx") {
                    found_inf = Some(path.clone());
                }
            }
        }

        if fname.ends_with(".cat") && !fname.contains("intcdaud") {
            found_cat = Some(path.clone());
        }

        let is_coinstaller = fname.starts_with("igfxcoin") && fname.ends_with(".dll");
        let should_keep = CORE_WDDM_FILES.contains(&fname.as_str())
            || is_coinstaller
            || (keep_ui && OPTIONAL_UI_FILES.contains(&fname.as_str()));

        if should_keep {
            fs::copy(path, out_dir.join(path.file_name().unwrap()))?;
            println!("[+] Kept core file: {}", fname);
        }
    }

    let src_inf = found_inf.with_context(|| {
        format!(
            "Could not find an Intel Display .inf file (igdlh64.inf or kit*.inf) inside '{}'. Make sure you extracted the zip/exe first.",
            src_dir.display()
        )
    })?;

    let dst_inf = out_dir.join("igdlh64.inf");
    fs::copy(&src_inf, &dst_inf)?;
    println!(
        "[+] Imported display INF: {} -> igdlh64.inf",
        src_inf.file_name().unwrap().to_string_lossy()
    );

    if let Some(ref cat) = found_cat {
        println!(
            "[+] Found original WHQL catalog: {}",
            cat.file_name().unwrap().to_string_lossy()
        );
    }

    Ok(StrippedPackage {
        inf_path: dst_inf,
        orig_whql_cat: found_cat,
    })
}
