use anyhow::{Context, Result};
use std::ffi::c_void;
use std::fs;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::process::Command;

#[repr(C)]
struct Guid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

const DRIVER_ACTION_VERIFY: Guid = Guid {
    data1: 0xF750E6C3,
    data2: 0x38EE,
    data3: 0x11D1,
    data4: [0x85, 0xE5, 0x00, 0xC0, 0x4F, 0xC2, 0x95, 0xEE],
};

#[link(name = "wintrust")]
extern "system" {
    fn CryptCATCDFOpen(
        pwszFilePath: *const u16,
        pfnParseError: *const c_void,
    ) -> *mut c_void;

    fn CryptCATCDFEnumCatAttributes(
        pCDF: *mut c_void,
        pPrevAttr: *mut c_void,
        pfnParseError: *const c_void,
    ) -> *mut c_void;

    fn CryptCATCDFEnumMembersByCDFTagEx(
        pCDF: *mut c_void,
        pwszPrevCDFTag: *mut u16,
        pfnParseError: *const c_void,
        ppMember: *mut *mut c_void,
        fContinueOnError: i32,
        pvReserved: *mut c_void,
    ) -> *mut u16;

    fn CryptCATCDFEnumAttributesWithCDFTag(
        pCDF: *mut c_void,
        pwszMemberTag: *mut u16,
        pMember: *mut c_void,
        pPrevAttr: *mut c_void,
        pfnParseError: *const c_void,
    ) -> *mut c_void;

    fn CryptCATCDFClose(pCDF: *mut c_void) -> i32;

    fn CryptCATAdminAcquireContext(
        phCatAdmin: *mut isize,
        pgSubsystem: *const Guid,
        dwFlags: u32,
    ) -> i32;

    fn CryptCATAdminAddCatalog(
        hCatAdmin: isize,
        pwszCatalogFile: *const u16,
        pwszSelectBaseName: *const u16,
        dwFlags: u32,
    ) -> isize;

    fn CryptCATAdminReleaseCatalogContext(
        hCatAdmin: isize,
        hCatInfo: isize,
        dwFlags: u32,
    ) -> i32;

    fn CryptCATAdminReleaseContext(
        hCatAdmin: isize,
        dwFlags: u32,
    ) -> i32;
}

fn to_wide(s: &str) -> Vec<u16> {
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

pub fn register_catalog_in_catroot(cat_path: &Path, base_name: &str) -> bool {
    if !cat_path.exists() {
        return false;
    }
    let abs_cat = cat_path.canonicalize().unwrap_or_else(|_| cat_path.to_path_buf());
    let cat_wide = to_wide(&abs_cat.to_string_lossy());
    let name_wide = to_wide(base_name);

    unsafe {
        let mut h_admin: isize = 0;
        if CryptCATAdminAcquireContext(&mut h_admin, &DRIVER_ACTION_VERIFY, 0) == 0 {
            return false;
        }
        let h_cat = CryptCATAdminAddCatalog(
            h_admin,
            cat_wide.as_ptr(),
            name_wide.as_ptr(),
            0,
        );
        let ok = h_cat != 0;
        if ok {
            CryptCATAdminReleaseCatalogContext(h_admin, h_cat, 0);
        }
        CryptCATAdminReleaseContext(h_admin, 0);
        ok
    }
}

pub fn generate_cat_natively(driver_dir: &Path) -> Result<()> {
    let abs_dir = driver_dir.canonicalize().unwrap_or_else(|_| driver_dir.to_path_buf());
    let clean_dir_str = abs_dir
        .to_string_lossy()
        .trim_start_matches(r"\\?\")
        .to_string();

    let cdf_path = abs_dir.join("igdlh.cdf");
    let cat_path = abs_dir.join("igdlh.cat");
    if cat_path.exists() {
        let _ = fs::remove_file(&cat_path);
    }

    let mut cdf_lines = vec![
        "[CatalogHeader]".to_string(),
        "Name=igdlh.cat".to_string(),
        format!("ResultDir={}", clean_dir_str),
        "PublicVersion=0x0000001".to_string(),
        "EncodingType=0x00010001".to_string(),
        "PageHashes=true".to_string(),
        "CATATTR1=0x10010001:OSAttr:2:6.1,2:6.2,2:10.0".to_string(),
        String::new(),
        "[CatalogFiles]".to_string(),
    ];

    let mut idx = 0;
    for entry in fs::read_dir(&abs_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            let fname = path.file_name().unwrap().to_string_lossy().to_string();
            let low = fname.to_lowercase();
            if low.ends_with(".cat") || low.ends_with(".cdf") {
                continue;
            }
            let file_clean = path
                .to_string_lossy()
                .trim_start_matches(r"\\?\")
                .to_string();
            cdf_lines.push(format!("<HASH>member{}=\"{}\"", idx, file_clean));
            cdf_lines.push(format!(
                "<HASH>member{}ATTR1=0x10010001:File:{}",
                idx, fname
            ));
            idx += 1;
        }
    }

    fs::write(&cdf_path, cdf_lines.join("\r\n"))?;

    let cdf_clean_str = cdf_path
        .to_string_lossy()
        .trim_start_matches(r"\\?\")
        .to_string();
    let cdf_wide = to_wide(&cdf_clean_str);

    unsafe {
        let p_cdf = CryptCATCDFOpen(cdf_wide.as_ptr(), std::ptr::null());
        if p_cdf.is_null() {
            anyhow::bail!("CryptCATCDFOpen failed to initialize catalog builder");
        }

        let mut p_attr: *mut c_void = std::ptr::null_mut();
        loop {
            p_attr = CryptCATCDFEnumCatAttributes(p_cdf, p_attr, std::ptr::null());
            if p_attr.is_null() {
                break;
            }
        }

        let mut p_member_tag: *mut u16 = std::ptr::null_mut();
        let mut p_member: *mut c_void = std::ptr::null_mut();
        loop {
            p_member_tag = CryptCATCDFEnumMembersByCDFTagEx(
                p_cdf,
                p_member_tag,
                std::ptr::null(),
                &mut p_member,
                1,
                std::ptr::null_mut(),
            );
            if p_member_tag.is_null() {
                break;
            }
            let mut p_mem_attr: *mut c_void = std::ptr::null_mut();
            loop {
                p_mem_attr = CryptCATCDFEnumAttributesWithCDFTag(
                    p_cdf,
                    p_member_tag,
                    p_member,
                    p_mem_attr,
                    std::ptr::null(),
                );
                if p_mem_attr.is_null() {
                    break;
                }
            }
        }

        CryptCATCDFClose(p_cdf);
    }

    let _ = fs::remove_file(&cdf_path);
    println!("[+] Built fresh Windows 10/11 security catalog (igdlh.cat) natively via WinTrust.");
    Ok(())
}

pub fn sign_and_install_driver(
    driver_dir: &Path,
    orig_whql_cat: Option<&Path>,
    enable_testmode: bool,
    install_now: bool,
) -> Result<()> {
    if let Some(whql_cat) = orig_whql_cat {
        if register_catalog_in_catroot(whql_cat, "igdlh_intel_whql.cat") {
            println!("[+] Registered original Microsoft WHQL catalog (igdlh_intel_whql.cat) in Windows CatRoot.");
        }
    }

    generate_cat_natively(driver_dir)?;

    if enable_testmode {
        let _ = Command::new("bcdedit").args(["/set", "testsigning", "on"]).status();
        let _ = Command::new("bcdedit").args(["/set", "nointegritychecks", "on"]).status();
    }

    let abs_dir = driver_dir.canonicalize().unwrap_or_else(|_| driver_dir.to_path_buf());
    let clean_dir = abs_dir
        .to_string_lossy()
        .trim_start_matches(r"\\?\")
        .to_string();

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

        if (Test-Path "{0}\igdlh.cat") {{
            Set-AuthenticodeSignature -FilePath "{0}\igdlh.cat" -Certificate $cert -HashAlgorithm SHA256 | Out-Null
        }}
        "#,
        clean_dir
    );

    let status = Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &ps_script])
        .status()
        .context("Failed to execute PowerShell Authenticode signing")?;

    if status.success() {
        println!("[+] Signed igdlh.cat with local Trusted Root & TrustedPublisher certificate.");
    }

    let new_cat = abs_dir.join("igdlh.cat");
    if register_catalog_in_catroot(&new_cat, "igdlh.cat") {
        println!("[+] Registered signed igdlh.cat into Windows CatRoot.");
    }

    if install_now {
        let inf_path = abs_dir.join("igdlh64.inf");
        let clean_inf = inf_path
            .to_string_lossy()
            .trim_start_matches(r"\\?\")
            .to_string();
        println!("[*] Installing signed driver package via pnputil...");
        Command::new("pnputil")
            .args(["/add-driver", &clean_inf, "/install"])
            .status()?;
    }
    Ok(())
}
