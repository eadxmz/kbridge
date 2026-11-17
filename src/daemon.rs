use crate::color::{apply_gamma_ramp, read_intel_cui_color_from_registry, ColorConfig};
use crate::os_tweaks::apply_live_gpu_registry_flags;
use anyhow::Result;
use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::thread;
use std::time::Duration;
use windows_sys::Win32::Foundation::{CloseHandle, HWND};
use windows_sys::Win32::Graphics::Gdi::{DEVMODEW, DM_PELSHEIGHT, DM_PELSWIDTH};
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows_sys::Win32::System::Threading::{
    OpenProcess, SetPriorityClass, SetProcessAffinityMask, HIGH_PRIORITY_CLASS,
    PROCESS_SET_INFORMATION,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

const ENUM_CURRENT_SETTINGS: u32 = 0xFFFF_FFFF;
const CDS_UPDATEREGISTRY: u32 = 0x0000_0001;
const CDS_RESET: u32 = 0x4000_0000;

#[link(name = "user32")]
extern "system" {
    fn EnumDisplaySettingsW(
        lpszDeviceName: *const u16,
        iModeNum: u32,
        lpDevMode: *mut DEVMODEW,
    ) -> i32;

    fn ChangeDisplaySettingsExW(
        lpszDeviceName: *const u16,
        lpDevMode: *const DEVMODEW,
        hwnd: HWND,
        dwflags: u32,
        lParam: *const c_void,
    ) -> i32;
}

#[link(name = "ntdll")]
extern "system" {
    fn NtSetTimerResolution(
        DesiredResolution: u32,
        SetResolution: u8,
        CurrentResolution: *mut u32,
    ) -> i32;

    fn NtSetSystemInformation(
        SystemInformationClass: u32,
        SystemInformation: *const c_void,
        SystemInformationLength: u32,
    ) -> i32;
}

const SYSTEM_MEMORY_LIST_INFORMATION: u32 = 80;
const MEMORY_PURGE_STANDBY_LIST: u32 = 4;

pub fn lock_kernel_timer_05ms(enable: bool) {
    let mut current: u32 = 0;
    unsafe {
        if enable {
            NtSetTimerResolution(5000, 1, &mut current);
        } else {
            NtSetTimerResolution(156250, 0, &mut current);
        }
    }
}

pub fn check_and_purge_standby_list(threshold_mb: u64) {
    unsafe {
        let mut mem: MEMORYSTATUSEX = zeroed();
        mem.dwLength = size_of::<MEMORYSTATUSEX>() as u32;
        if GlobalMemoryStatusEx(&mut mem) != 0 {
            let free_mb = mem.ullAvailPhys / (1024 * 1024);
            if free_mb < threshold_mb {
                let cmd: u32 = MEMORY_PURGE_STANDBY_LIST;
                let _ = NtSetSystemInformation(
                    SYSTEM_MEMORY_LIST_INFORMATION,
                    &cmd as *const _ as *const c_void,
                    size_of::<u32>() as u32,
                );
            }
        }
    }
}

pub fn set_display_resolution_or_reset(width: Option<u32>, height: Option<u32>) -> bool {
    unsafe {
        let mut dm: DEVMODEW = zeroed();
        dm.dmSize = size_of::<DEVMODEW>() as u16;
        if EnumDisplaySettingsW(std::ptr::null(), ENUM_CURRENT_SETTINGS, &mut dm) == 0 {
            return false;
        }
        if let (Some(w), Some(h)) = (width, height) {
            dm.dmPelsWidth = w;
            dm.dmPelsHeight = h;
            dm.dmFields = DM_PELSWIDTH | DM_PELSHEIGHT;
        }
        ChangeDisplaySettingsExW(
            std::ptr::null(),
            &dm,
            std::ptr::null_mut(),
            CDS_RESET | CDS_UPDATEREGISTRY,
            std::ptr::null(),
        ) == 0
    }
}

pub fn switch_gpu_profile(overdrive: bool, color_cfg: &ColorConfig) -> Result<()> {
    apply_live_gpu_registry_flags(overdrive)?;
    set_display_resolution_or_reset(None, None);
    thread::sleep(Duration::from_millis(150));
    apply_gamma_ramp(color_cfg);
    Ok(())
}

pub fn boost_foreground_process(physical_core_mask: usize) {
    unsafe {
        let hwnd: HWND = GetForegroundWindow();
        if hwnd.is_null() {
            return;
        }
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 || pid == std::process::id() {
            return;
        }
        let h_proc = OpenProcess(PROCESS_SET_INFORMATION, 0, pid);
        if !h_proc.is_null() {
            let _ = SetPriorityClass(h_proc, HIGH_PRIORITY_CLASS);
            let _ = SetProcessAffinityMask(h_proc, physical_core_mask);
            CloseHandle(h_proc);
        }
    }
}

pub fn run_daemon_loop(fallback_color: ColorConfig) {
    println!("[*] KBridge Background Guard Active (Color LUT Lock + 0.5ms Timer + Standby RAM Purger)...");
    println!("[*] You can minimize this window. It will start silently in the background on future reboots.");
    lock_kernel_timer_05ms(true);

    let mut last_hwnd: isize = 0;
    loop {
        let active_color = read_intel_cui_color_from_registry().unwrap_or(fallback_color);
        apply_gamma_ramp(&active_color);

        let cur_hwnd = unsafe { GetForegroundWindow() } as isize;
        if cur_hwnd != 0 && cur_hwnd != last_hwnd {
            last_hwnd = cur_hwnd;
            boost_foreground_process(0b0101);
        }

        check_and_purge_standby_list(500);

        thread::sleep(Duration::from_secs(3));
    }
}
