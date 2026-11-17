mod color;
mod daemon;
mod inf_mod;
mod os_tweaks;
mod patcher;
mod signer;

use anyhow::Result;
use color::ColorConfig;
use std::env;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

fn prompt(msg: &str) -> String {
    print!("{}", msg);
    let _ = io::stdout().flush();
    let mut input = String::new();
    let _ = io::stdin().read_line(&mut input);
    input.trim().to_string()
}

fn pause() {
    println!();
    let _ = prompt("Press Enter to return to the menu...");
}

fn resolve_driver_input_path() -> Option<PathBuf> {
    let default_candidates = [r".\win64_152258", r"C:\win64_152258"];
    for cand in &default_candidates {
        if Path::new(cand).exists() {
            let ans = prompt(&format!(
                "Found driver folder '{}'. Use this folder? [Y/n]: ",
                cand
            ));
            if ans.is_empty() || ans.eq_ignore_ascii_case("y") {
                return Some(PathBuf::from(cand));
            }
        }
    }

    let raw = prompt("Enter or drag-and-drop extracted Intel 15.22.58.2993 folder path: ");
    let cleaned = raw.trim_matches('"').trim();
    if cleaned.is_empty() {
        return None;
    }
    let path = PathBuf::from(cleaned);
    if !path.exists() {
        println!("[!] Path does not exist: {}", path.display());
        return None;
    }
    Some(path)
}

fn run_driver_workflow(install_now: bool) -> Result<()> {
    let Some(input_dir) = resolve_driver_input_path() else {
        println!("[!] Cancelled: No valid driver folder provided.");
        return Ok(());
    };

    let out_raw = prompt("Output folder [default: .\\patched_driver]: ");
    let out_dir = if out_raw.is_empty() {
        PathBuf::from("./patched_driver")
    } else {
        PathBuf::from(out_raw.trim_matches('"').trim())
    };

    let ui_ans = prompt("Keep Intel Control Panel UI (GfxUI.exe - 0 MB idle RAM)? [Y/n]: ");
    let keep_ui = ui_ans.is_empty() || ui_ans.eq_ignore_ascii_case("y");

    println!("\n[*] Stripping bloatware & keeping core WDDM files...");
    let orig_whql_cat = patcher::strip_driver_package(&input_dir, &out_dir, keep_ui)?;

    println!("[*] Unlocking Windows 10/11 target & baking 11 performance DWORDs into INF...");
    inf_mod::patch_inf_for_win10_11(&out_dir.join("igdlh64.inf"), keep_ui)?;

    println!("[*] Preserving original WHQL signature on igdkmd64.sys (No Test Mode required)...");
    signer::sign_and_install_driver(&out_dir, orig_whql_cat.as_deref(), false, install_now)?;

    color::disable_windows_color_conflicts();
    println!("\n[✓] Driver processing complete!");
    Ok(())
}

fn run_display_preset_menu() {
    println!("\n--- Display Resolution & Gamma Presets ---");
    println!("  [1] Native 1366x768  | Balanced Color (Gamma 1.08, Contrast 1.05)");
    println!("  [2] Gaming 1024x576  | 16:9 Scaled + Vibrance (Gamma 1.18, Contrast 1.10)");
    println!("  [3] Ultra  960x540   | 16:9 Scaled + Vibrance (Gamma 1.22, Contrast 1.12)");
    println!("  [4] Custom Resolution & Gamma");
    let choice = prompt("Select preset [1-4]: ");

    let (w, h, gamma, contrast) = match choice.as_str() {
        "1" => (1366, 768, 1.08, 1.05),
        "2" => (1024, 576, 1.18, 1.10),
        "3" => (960, 540, 1.22, 1.12),
        "4" => {
            let w: u32 = prompt("Width (e.g. 1366): ").parse().unwrap_or(1366);
            let h: u32 = prompt("Height (e.g. 768): ").parse().unwrap_or(768);
            let g: f32 = prompt("Gamma (e.g. 1.12): ").parse().unwrap_or(1.12);
            let c: f32 = prompt("Contrast (e.g. 1.05): ").parse().unwrap_or(1.05);
            (w, h, g, c)
        }
        _ => {
            println!("[!] Invalid selection.");
            return;
        }
    };

    if daemon::set_display_resolution_or_reset(Some(w), Some(h)) {
        let cfg = ColorConfig {
            gamma,
            contrast,
            ..Default::default()
        };
        color::apply_gamma_ramp(&cfg);
        println!("[+] Applied {}x{} (Gamma {:.2}, Contrast {:.2}).", w, h, gamma, contrast);
    } else {
        println!("[!] Display mode {}x{} was rejected by the display adapter.", w, h);
    }
}

fn setup_and_run_daemon(register_startup: bool) -> Result<()> {
    let g_str = prompt("Desired Gamma [default: 1.10]: ");
    let gamma: f32 = if g_str.is_empty() {
        1.10
    } else {
        g_str.parse().unwrap_or(1.10)
    };

    let c_str = prompt("Desired Contrast [default: 1.05]: ");
    let contrast: f32 = if c_str.is_empty() {
        1.05
    } else {
        c_str.parse().unwrap_or(1.05)
    };

    if register_startup {
        let exe = env::current_exe()?;
        let status = Command::new("schtasks")
            .args([
                "/Create",
                "/F",
                "/RL",
                "HIGHEST",
                "/SC",
                "ONLOGON",
                "/TN",
                "KBridgeOptDaemon",
                "/TR",
                &format!(
                    "\"{}\" --daemon --gamma {} --contrast {}",
                    exe.display(),
                    gamma,
                    contrast
                ),
            ])
            .status()?;
        if status.success() {
            println!("[+] Registered KBridgeOptDaemon to run automatically at Windows Logon.");
        }
    }

    color::disable_windows_color_conflicts();
    let cfg = ColorConfig {
        gamma,
        contrast,
        ..Default::default()
    };
    daemon::run_daemon_loop(cfg);
    Ok(())
}

fn parse_background_daemon_args() -> Option<ColorConfig> {
    let args: Vec<String> = env::args().collect();
    if !args.iter().any(|a| a == "--daemon") {
        return None;
    }
    let mut cfg = ColorConfig {
        gamma: 1.10,
        contrast: 1.05,
        ..Default::default()
    };
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--gamma" if i + 1 < args.len() => {
                cfg.gamma = args[i + 1].parse().unwrap_or(1.10);
                i += 1;
            }
            "--brightness" if i + 1 < args.len() => {
                cfg.brightness = args[i + 1].parse().unwrap_or(0.0);
                i += 1;
            }
            "--contrast" if i + 1 < args.len() => {
                cfg.contrast = args[i + 1].parse().unwrap_or(1.05);
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    Some(cfg)
}

fn main() -> Result<()> {
    // Silent background launch when triggered by Windows Task Scheduler (`--daemon`)
    if let Some(bg_cfg) = parse_background_daemon_args() {
        color::disable_windows_color_conflicts();
        daemon::run_daemon_loop(bg_cfg);
        return Ok(());
    }

    loop {
        println!("\n==================================================================");
        println!("  KBridge - Intel HD Graphics (15.22.58.2993) Win10/11 Optimizer");
        println!("==================================================================");
        println!("  [1] Patch, Sign & Install Driver (Normal Mode - No Test Mode)");
        println!("  [2] Patch & Sign Driver Only (Export to folder without installing)");
        println!("  [3] Apply Windows 10/11 OS, MSI, FSE & Color Reset Fixes");
        println!("  [4] Switch Live GPU Profile: OVERDRIVE (Max Clocks, RC6 Off)");
        println!("  [5] Switch Live GPU Profile: ECO (Stock Clocks, Power Saving)");
        println!("  [6] Quick Display Resolution & Gamma Preset");
        println!("  [7] Register Daemon at Windows Startup & Start Now");
        println!("  [8] Start Background Daemon Now (Current Session Only)");
        println!("  [0] Exit");
        println!("==================================================================");

        let choice = prompt("Select an option [0-8]: ");
        let res: Result<()> = match choice.as_str() {
            "1" => run_driver_workflow(true),
            "2" => run_driver_workflow(false),
            "3" => {
                color::disable_windows_color_conflicts();
                os_tweaks::apply_live_gpu_registry_flags(true)?;
                os_tweaks::apply_win10_11_os_tweaks()?;
                Ok(())
            }
            "4" => {
                daemon::switch_gpu_profile(true, &ColorConfig::default())?;
                println!("[+] Live GPU profile set to OVERDRIVE (RP0 locked, RC6 disabled).");
                Ok(())
            }
            "5" => {
                daemon::switch_gpu_profile(false, &ColorConfig::default())?;
                println!("[+] Live GPU profile set to ECO (Dynamic clocking & RC6 restored).");
                Ok(())
            }
            "6" => {
                run_display_preset_menu();
                Ok(())
            }
            "7" => setup_and_run_daemon(true),
            "8" => setup_and_run_daemon(false),
            "0" => break,
            _ => {
                println!("[!] Invalid option. Please enter a number between 0 and 8.");
                Ok(())
            }
        };

        if let Err(e) = res {
            println!("\n[ERROR] {}", e);
        }
        pause();
    }

    Ok(())
}
