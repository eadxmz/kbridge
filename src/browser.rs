use anyhow::Result;
use std::env;
use std::fs;
use std::path::PathBuf;

pub fn apply_firefox_1440p60_tweaks(is_haswell: bool) -> Result<()> {
    let Ok(appdata) = env::var("APPDATA") else {
        println!("[!] APPDATA environment variable not found.");
        return Ok(());
    };

    let profiles_dir = PathBuf::from(appdata).join(r"Mozilla\Firefox\Profiles");
    if !profiles_dir.exists() {
        println!("[i] No Firefox profile directory detected. If using Chrome/Edge, enable Hardware Acceleration + enhanced-h264ify.");
        return Ok(());
    }

    let dcomp_val = if is_haswell { "true" } else { "false" };
    let user_js_block = format!(
        r#"// --- KBridge v0.6.0 1440p60 Hardware Video & Frame-Pacing Tweaks ---
user_pref("media.hardware-video-decoding.enabled", true);
user_pref("media.hardware-video-decoding.force-enabled", true);
user_pref("media.wmf.dxva.enabled", true);
user_pref("media.wmf.dxva.d3d11.enabled", true);
user_pref("media.av1.enabled", false);
user_pref("gfx.webrender.all", true);
user_pref("gfx.webrender.compositor", {0});
user_pref("gfx.webrender.dcomp-win.enabled", {0});
user_pref("layers.gpu-process.enabled", true);
user_pref("media.gpu-process-decoder", true);
user_pref("media.ffmpeg.vaapi.enabled", false);
user_pref("media.rdd-process.enabled", true);
user_pref("media.cache_readahead_limit", 180);
user_pref("media.cache_resume_threshold", 90);
"#,
        dcomp_val
    );

    let mut updated = 0usize;
    for entry in fs::read_dir(&profiles_dir)?.flatten() {
        let path = entry.path();
        if path.is_dir() && path.join("prefs.js").exists() {
            let target = path.join("user.js");
            let mut existing = fs::read_to_string(&target).unwrap_or_default();
            if !existing.contains("KBridge v0.6.0 1440p60") {
                if !existing.is_empty() && !existing.ends_with('\n') {
                    existing.push('\n');
                }
                existing.push_str(&user_js_block);
                fs::write(&target, existing)?;
                println!("[+] Tuned Firefox profile: {}", target.display());
                updated += 1;
            } else {
                println!("[i] Profile already tuned: {}", target.display());
            }
        }
    }

    if updated > 0 {
        println!("[+] Restart Firefox and ensure 'enhanced-h264ify' (blocking AV1/VP9) is enabled for 1440p60.");
    }
    Ok(())
}
