mod browser;
mod color;
mod daemon;
mod framegen;
mod inf_mod;
mod os_tweaks;
mod patcher;
mod signer;

use anyhow::Result;
use color::ColorConfig;
use framegen::{FrameGenMode, LatencyMode};
use std::env;
use std::io::{self, Write};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const DETACHED_PROCESS: u32 = 0x0000_0008;

#[link(name = "kernel32")]
extern "system" {
    fn FreeConsole() -> i32;
}

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
    let default_candidates = [
        r".\win7vista_64_152258",
        r".\win64_152258",
        r".\win64_1540",
        r".\win64_1536",
        r"C:\win7vista_64_152258",
        r"C:\win64_1540",
    ];
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

    let raw = prompt("Enter or drag-and-drop extracted Intel driver folder (Ironlake 15.22 or Haswell HD 4400 15.36/15.40): ");
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

    let ui_ans = prompt("Keep Intel Control Panel UI (GfxUI / GfxUIEx - 0 MB idle RAM)? [Y/n]: ");
    let keep_ui = ui_ans.is_empty() || ui_ans.eq_ignore_ascii_case("y");

    println!("\n[*] Scanning package, detecting GPU generation & preserving core WDDM + Media files...");
    let pkg = patcher::strip_driver_package(&input_dir, &out_dir, keep_ui)?;

    println!("[*] Patching INF for Win10/11, clearing ExcludeFromSelect & baking 25+ Max Performance DWORDs...");
    inf_mod::patch_inf_for_win10_11(&pkg.inf_path, &out_dir, keep_ui, pkg.is_haswell)?;

    println!("[*] Building & signing security catalog in Normal Mode (No Test Mode required)...");
    signer::sign_and_install_driver(&out_dir, pkg.orig_whql_cat.as_deref(), false, install_now)?;

    color::disable_windows_color_conflicts();
    println!("\n[✓] Driver processing complete! Output saved to: {}", out_dir.display());
    if !install_now {
        println!("[i] Next: Run Option [1] to install directly, or install via Device Manager -> Have Disk.");
    } else {
        println!("[i] Next: Reboot if prompted, then run Option [3] to apply MAX 1440p60 OS & Daemon tweaks.");
    }
    Ok(())
}

fn run_framegen_menu() -> Result<()> {
    println!("\n--- KBridge Dual-GPU Offload Frame Gen (Intel HD 4400 Only) ---");
    println!("  Select Optical Flow Engine:");
    println!("  [1] Pyramid Optical Flow (2x — Best 3D Games, Coarse 1/8 -> Fine 1/4 + HUD Lock)");
    println!("  [2] Ultra-Lite Lucas-Kanade (2x — Single-Pass PS 4.0, Lowest GPU Load)");
    println!("  [3] Adaptive Video Smoother (24/30 -> 60 fps — Scene-Cut Guard for YouTube/Anime)");
    let m_choice = prompt("Select engine [1-3, default 1]: ");
    let mode = match m_choice.as_str() {
        "2" => FrameGenMode::UltraLiteLucasKanade,
        "3" => FrameGenMode::AdaptiveVideoSmoother,
        _ => FrameGenMode::PyramidOpticalFlow,
    };

    println!("\n  Select Input Latency Technique:");
    println!("  [1] ZeroLag-X Extrapolation (0-Frame Hold + Mouse-Delta Assist — Ultra-Low Latency)");
    println!("  [2] Classic Interpolation   (1-Frame Hold — Smoothest Motion for Movies/Slow Games)");
    let l_choice = prompt("Select latency mode [1-2, default 1]: ");
    let latency = match l_choice.as_str() {
        "2" => LatencyMode::ClassicInterpolation,
        _ => LatencyMode::ZeroLagExtrapolation,
    };

    framegen::run_hd4400_framegen(mode, latency)
}

fn run_display_preset_menu() {
    println!("\n--- Display Resolution & Gamma Presets ---");
    println!("  [1] Native 1366x768  | Balanced Color (Gamma 1.08, Contrast 1.05)");
    println!("  [2] FullHD 1920x1080 | Crisp Color    (Gamma 1.06, Contrast 1.04)");
    println!("  [3] Gaming 1024x576  | 16:9 Scaled + Vibrance (Gamma 1.18, Contrast 1.10)");
    println!("  [4] Ultra  960x540   | 16:9 Scaled + Vibrance (Gamma 1.22, Contrast 1.12)");
    println!("  [5] Custom Resolution & Gamma");
    let choice = prompt("Select preset [1-5]: ");

    let (w, h, gamma, contrast) = match choice.as_str() {
        "1" => (1366, 768, 1.08, 1.05),
        "2" => (1920, 1080, 1.06, 1.04),
        "3" => (1024, 576, 1.18, 1.10),
        "4" => (960, 540, 1.22, 1.12),
        "5" => {
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
        color::apply_gamma_ramp_force(&cfg);
        println!("[+] Applied {}x{} (Gamma {:.2}, Contrast {:.2}).", w, h, gamma, contrast);
    } else {
        println!("[!] Display mode {}x{} was rejected by the display adapter.", w, h);
    }
}

fn register_instant_hidden_startup_task(exe_path: &Path, gamma: f32, contrast: f32) -> Result<()> {
    let ps_script = format!(
        r#"
        $ErrorActionPreference = 'Stop'
        $action = New-ScheduledTaskAction -Execute '{0}' -Argument '--daemon --gamma {1} --contrast {2}'
        $trigger = New-ScheduledTaskTrigger -AtLogOn
        $settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit ([TimeSpan]::Zero) -Priority 0 -Hidden
        $principal = New-ScheduledTaskPrincipal -GroupId 'S-1-5-32-544' -RunLevel Highest
        Register-ScheduledTask -TaskName 'KBridgeOptDaemon' -Action $action -Trigger $trigger -Settings $settings -Principal $principal -Force | Out-Null
        "#,
        exe_path.display(),
        gamma,
        contrast
    );

    let status = Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &ps_script])
        .status()?;

    if status.success() {
        println!("[+] Registered instant Priority-0 hidden startup task (KBridgeOptDaemon).");
    } else {
        let _ = Command::new("schtasks")
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
                    exe_path.display(),
                    gamma,
                    contrast
                ),
            ])
            .status();
        println!("[+] Registered KBridgeOptDaemon logon task via schtasks.");
    }
    Ok(())
}

fn spawn_headless_daemon_now(exe_path: &Path, gamma: f32, contrast: f32) -> Result<()> {
    let my_pid = std::process::id();
    let _ = Command::new("taskkill")
        .args([
            "/F",
            "/FI",
            &format!("PID ne {}", my_pid),
            "/IM",
            "kbridge.exe",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output();

    Command::new(exe_path)
        .args([
            "--daemon",
            "--gamma",
            &gamma.to_string(),
            "--contrast",
            &contrast.to_string(),
        ])
        .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
        .spawn()?;

    println!("[+] KBridge v0.6.0 Guard is now running invisibly in the background (0 windows)!");
    Ok(())
}

fn setup_and_run_daemon(register_startup: bool) -> Result<()> {
    println!("\n--- Invisible 1440p60 Guard & Color LUT Lock Setup ---");
    let g_str = prompt("Desired Gamma [default: 1.08]: ");
    let gamma: f32 = if g_str.is_empty() {
        1.08
    } else {
        g_str.parse().unwrap_or(1.08)
    };

    let c_str = prompt("Desired Contrast [default: 1.05]: ");
    let contrast: f32 = if c_str.is_empty() {
        1.05
    } else {
        c_str.parse().unwrap_or(1.05)
    };

    color::disable_windows_color_conflicts();
    let cfg = ColorConfig {
        gamma,
        contrast,
        ..Default::default()
    };
    color::apply_gamma_ramp_force(&cfg);

    let exe = env::current_exe()?;
    if register_startup {
        register_instant_hidden_startup_task(&exe, gamma, contrast)?;
    }

    spawn_headless_daemon_now(&exe, gamma, contrast)?;
    Ok(())
}

fn parse_background_daemon_args() -> Option<ColorConfig> {
    let args: Vec<String> = env::args().collect();
    if !args.iter().any(|a| a == "--daemon") {
        return None;
    }
    let mut cfg = ColorConfig {
        gamma: 1.08,
        contrast: 1.05,
        ..Default::default()
    };
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--gamma" if i + 1 < args.len() => {
                cfg.gamma = args[i + 1].parse().unwrap_or(1.08);
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
    if let Some(bg_cfg) = parse_background_daemon_args() {
        unsafe {
            FreeConsole();
        }
        color::disable_windows_color_conflicts();
        daemon::run_daemon_loop(bg_cfg);
        return Ok(());
    }

    let (_, is_haswell) = os_tweaks::detect_active_intel_gpu_class_key();
    let gpu_label = if is_haswell {
        "Haswell HD 4400 Family (Gen 7.5 — Frame Gen Ready)"
    } else {
        "Ironlake HD Graphics (1st-Gen)"
    };

    loop {
        println!("\n======================================================================");
        println!("  KBridge v0.6.0 ULTIMATE - Intel HD Graphics + ZeroLag-X Frame Gen");
        println!("  Detected Active GPU: {}", gpu_label);
        println!("======================================================================");
        println!("  --- STEP 1: MULTI-GEN DRIVER PATCH & INSTALL ---");
        println!("  [1] Patch, Sign & Install Driver (Ironlake & HD 4400 - No Test Mode)");
        println!("  [2] Patch & Sign Driver Only (Export folder without installing)");
        println!();
        println!("  --- STEP 2: RUN ONCE AFTER DRIVER INSTALL (MAX 1440p60 TWEAKS) ---");
        println!("  [3] Apply ALL Max GPU/OS/Kernel/Power Tweaks + Invisible Guard");
        println!();
        println!("  --- FRAME GENERATION & DAILY TOOLS ---");
        println!("  [4] GPU Mode: OVERDRIVE (RP0 Boost, RC6/DPST/PSR Off, 512MB GMM)");
        println!("  [5] GPU Mode: ECO (Stock Clocks, Power Saving - Battery/Cool)");
        println!("  [6] Change Display Resolution & Gamma Preset");
        println!("  [7] Apply 1440p60 Firefox Hardware-Decode Profile (user.js)");
        println!("  [8] Start Invisible Background Guard Now (This Session Only)");
        println!("  [9] Launch HD 4400 Dual-GPU Frame Gen (ZeroLag-X / Pyramid / LK / Video)");
        println!("  [0] Exit");
        println!("======================================================================");

        let choice = prompt("Select an option [0-9]: ");
        let res: Result<()> = match choice.as_str() {
            "1" => run_driver_workflow(true),
            "2" => run_driver_workflow(false),
            "3" => {
                color::disable_windows_color_conflicts();
                os_tweaks::apply_live_gpu_registry_flags(true)?;
                os_tweaks::apply_win10_11_os_tweaks()?;
                setup_and_run_daemon(true)
            }
            "4" => {
                daemon::switch_gpu_profile(true, &ColorConfig::default())?;
                println!("[+] Live GPU profile set to OVERDRIVE (RP0 locked, RC6/DPST/PSR off).");
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
            "7" => {
                browser::apply_firefox_1440p60_tweaks(is_haswell)?;
                Ok(())
            }
            "8" => setup_and_run_daemon(false),
            "9" => run_framegen_menu(),
            "0" => break,
            _ => {
                println!("[!] Invalid option. Please enter a number between 0 and 9.");
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
