mod color;
mod daemon;
mod inf_mod;
mod os_tweaks;
mod patcher;
mod signer;

use anyhow::Result;
use clap::{Parser, Subcommand};
use color::ColorConfig;
use std::path::PathBuf;
use std::process::Command;

#[derive(Parser)]
#[command(name = "kbridge", version, about = "Intel 15.22.58.2993 Win10/11 Patcher & Daemon")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Strip, patch INF for Win10/11, build & sign catalog (No Test Mode by default), and install
    PatchDriver {
        /// Path to the extracted original Intel 15.22.58.2993 driver folder
        #[arg(short, long)]
        input: PathBuf,
        /// Output directory for the patched driver package
        #[arg(short, long, default_value = "./patched_driver")]
        output: PathBuf,
        /// Keep GfxUI.exe so the original Intel Control Panel can still be opened manually
        #[arg(long, default_value_t = true)]
        keep_ui: bool,
        /// Immediately install the signed driver via pnputil
        #[arg(long, default_value_t = false)]
        install: bool,
        /// Optional: patch .sys/.dll bytes directly (requires Windows Test Mode)
        #[arg(long, default_value_t = false)]
        patch_binaries_testmode: bool,
    },
    /// Apply Windows 10/11 OS tweaks, MSI interrupts, FSE, and disable WCS color reset task
    OsTune,
    /// Toggle live GPU profile between Overdrive (RP0/No-RC6) and Eco without rebooting
    Profile {
        #[arg(value_parser = ["overdrive", "eco"])]
        mode: String,
    },
    /// Instantly switch resolution and apply custom gamma/contrast ramp
    Display {
        #[arg(short, long)]
        width: u32,
        #[arg(short, long)]
        height: u32,
        #[arg(short, long, default_value_t = 1.15)]
        gamma: f32,
    },
    /// Run the background daemon (Color LUT persistence, 0.5ms timer, Standby RAM purge)
    Daemon {
        #[arg(long, default_value_t = 1.08)]
        gamma: f32,
        #[arg(long, default_value_t = 0.0)]
        brightness: f32,
        #[arg(long, default_value_t = 1.05)]
        contrast: f32,
        /// Register the daemon as an elevated Windows Logon Scheduled Task
        #[arg(long, default_value_t = false)]
        register_startup: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::PatchDriver {
            input,
            output,
            keep_ui,
            install,
            patch_binaries_testmode,
        } => {
            let orig_whql_cat = patcher::strip_driver_package(&input, &output, keep_ui)?;
            inf_mod::patch_inf_for_win10_11(&output.join("igdlh64.inf"), keep_ui)?;
            if patch_binaries_testmode {
                patcher::patch_binaries(&output)?;
            } else {
                println!("[*] Preserving original Microsoft WHQL signature on igdkmd64.sys (No Test Mode required).");
            }
            signer::sign_and_install_driver(
                &output,
                orig_whql_cat.as_deref(),
                patch_binaries_testmode,
                install,
            )?;
            color::disable_windows_color_conflicts();
        }
        Commands::OsTune => {
            color::disable_windows_color_conflicts();
            os_tweaks::apply_live_gpu_registry_flags(true)?;
            os_tweaks::apply_win10_11_os_tweaks()?;
        }
        Commands::Profile { mode } => {
            let overdrive = mode == "overdrive";
            daemon::switch_gpu_profile(overdrive, &ColorConfig::default())?;
            println!("[+] Switched live GPU state to: {}", mode);
        }
        Commands::Display { width, height, gamma } => {
            daemon::set_display_resolution_or_reset(Some(width), Some(height));
            let cfg = ColorConfig { gamma, ..Default::default() };
            color::apply_gamma_ramp(&cfg);
        }
        Commands::Daemon { gamma, brightness, contrast, register_startup } => {
            if register_startup {
                let exe = std::env::current_exe()?;
                Command::new("schtasks")
                    .args([
                        "/Create", "/F", "/RL", "HIGHEST", "/SC", "ONLOGON",
                        "/TN", "KBridgeOptDaemon",
                        "/TR", &format!("\"{}\" daemon --gamma {} --brightness {} --contrast {}",
                            exe.display(), gamma, brightness, contrast),
                    ])
                    .status()?;
                println!("[+] Registered KBridgeOptDaemon to run at Windows Logon.");
            }
            color::disable_windows_color_conflicts();
            let cfg = ColorConfig {
                gamma,
                brightness,
                contrast,
                ..Default::default()
            };
            daemon::run_daemon_loop(cfg);
        }
    }
    Ok(())
}
