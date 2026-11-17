use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

pub fn sign_and_install_driver(driver_dir: &Path, install_now: bool) -> Result<()> {
    let _ = Command::new("bcdedit").args(["/set", "testsigning", "on"]).status();
    let _ = Command::new("bcdedit").args(["/set", "nointegritychecks", "on"]).status();

    let dir_str = driver_dir.to_string_lossy();
    let _ = Command::new("Inf2Cat.exe")
        .args([&format!("/driver:{}", dir_str), "/os:7_X64,8_X64,10_X64"])
        .status();

    let ps_script = format!(
        r#"
        $ErrorActionPreference = 'Stop'
        $certName = "KBridgeDriverRootCA"
        $cert = Get-ChildItem Cert:\LocalMachine\My | Where-Object {{ $_.Subject -eq "CN=$certName" }} | Select-Object -First 1
        if (-not $cert) {{
            $cert = New-SelfSignedCertificate -Type Custom -Subject "CN=$certName" `
                -KeyUsage DigitalSignature -KeyAlgorithm RSA -KeyLength 2048 `
                -CertStoreLocation "Cert:\LocalMachine\My" `
                -TextExtension @("2.5.29.37={{text}}1.3.6.1.5.5.7.3.3") -NotAfter (Get-Date).AddYears(10)
        }}
        $rootStore = New-Object System.Security.Cryptography.X509Certificates.X509Store "Root", "LocalMachine"
        $rootStore.Open("ReadWrite"); $rootStore.Add($cert); $rootStore.Close()
        $pubStore = New-Object System.Security.Cryptography.X509Certificates.X509Store "TrustedPublisher", "LocalMachine"
        $pubStore.Open("ReadWrite"); $pubStore.Add($cert); $pubStore.Close()

        Set-AuthenticodeSignature -FilePath "{0}\igdkmd64.sys" -Certificate $cert | Out-Null
        if (Test-Path "{0}\igdlh.cat") {{
            Set-AuthenticodeSignature -FilePath "{0}\igdlh.cat" -Certificate $cert | Out-Null
        }}
        "#,
        driver_dir.display()
    );

    let status = Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &ps_script])
        .status()
        .context("Failed to execute PowerShell Authenticode signing")?;

    if status.success() {
        println!("[+] Signed igdkmd64.sys and igdlh.cat with local Trusted Root certificate.");
    }

    if install_now {
        let inf_path = driver_dir.join("igdlh64.inf");
        println!("[*] Installing patched driver via pnputil...");
        Command::new("pnputil")
            .args(["/add-driver", &inf_path.to_string_lossy(), "/install"])
            .status()?;
    }
    Ok(())
}
