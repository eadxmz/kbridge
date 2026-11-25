use crate::color::{
    apply_gamma_ramp_force, read_intel_cui_color_from_registry, ColorConfig,
};
use crate::os_tweaks::apply_live_gpu_registry_flags;
use anyhow::Result;
use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStrExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{CloseHandle, HWND};
use windows_sys::Win32::Graphics::Gdi::{DEVMODEW, DM_PELSHEIGHT, DM_PELSWIDTH};
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, SetPriorityClass, SetProcessAffinityMask,
    ABOVE_NORMAL_PRIORITY_CLASS, HIGH_PRIORITY_CLASS, PROCESS_SET_INFORMATION,
};
use windows_sys::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetForegroundWindow, GetWindowThreadProcessId, PeekMessageW,
    TranslateMessage, EVENT_SYSTEM_FOREGROUND, MSG, PM_REMOVE, WINEVENT_OUTOFCONTEXT,
};

const ENUM_CURRENT_SETTINGS: u32 = 0xFFFF_FFFF;
const CDS_UPDATEREGISTRY: u32 = 0x0000_0001;
const CDS_RESET: u32 = 0x4000_0000;
const EVENT_OBJECT_FOCUS: u32 = 0x0000_8005;
const WINEVENT_SKIPOWNPROCESS: u32 = 0x0000_0002;
const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x0000_1000;
const TOKEN_ADJUST_PRIVILEGES: u32 = 0x0020;
const TOKEN_QUERY: u32 = 0x0008;
const SE_PRIVILEGE_ENABLED: u32 = 0x0000_0002;

static UI_EVENT_TRIGGERED: AtomicBool = AtomicBool::new(true);

#[repr(C)]
struct Luid {
    low_part: u32,
    high_part: i32,
}

#[repr(C)]
struct LuidAndAttributes {
    luid: Luid,
    attributes: u32,
}

#[repr(C)]
struct TokenPrivileges {
    privilege_count: u32,
    privileges: [LuidAndAttributes; 1],
}

#[link(name = "advapi32")]
extern "system" {
    fn OpenProcessToken(
        ProcessHandle: *mut c_void,
        DesiredAccess: u32,
        TokenHandle: *mut *mut c_void,
    ) -> i32;
    fn LookupPrivilegeValueW(
        lpSystemName: *const u16,
        lpName: *const u16,
        lpLuid: *mut Luid,
    ) -> i32;
    fn AdjustTokenPrivileges(
        TokenHandle: *mut c_void,
        DisableAllPrivileges: i32,
        NewState: *const TokenPrivileges,
        BufferLength: u32,
        PreviousState: *mut c_void,
        ReturnLength: *mut u32,
    ) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn QueryFullProcessImageNameW(
        hProcess: *mut c_void,
        dwFlags: u32,
        lpExeName: *mut u16,
        lpdwSize: *mut u32,
    ) -> i32;
}

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

const MULTITHREADED_MEDIA_EXES: &[&str] = &[
    "firefox.exe",
    "chrome.exe",
    "msedge.exe",
    "brave.exe",
    "opera.exe",
    "vivaldi.exe",
    "mpv.exe",
    "vlc.exe",
    "mpc-hc64.exe",
    "mpc-hc.exe",
    "mpc-be64.exe",
    "potplayermini64.exe",
    "wmplayer.exe",
    "obs64.exe",
];

unsafe extern "system" fn win_event_callback(
    _h_win_event_hook: HWINEVENTHOOK,
    _event: u32,
    _hwnd: HWND,
    _id_object: i32,
    _id_child: i32,
    _id_event_thread: u32,
    _dwms_event_time: u32,
) {
    UI_EVENT_TRIGGERED.store(true, Ordering::SeqCst);
}

fn enable_standby_purge_privilege() {
    let priv_name: Vec<u16> = std::ffi::OsStr::new("SeProfileSingleProcessPrivilege")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let mut h_token: *mut c_void = std::ptr::null_mut();
        if OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
            &mut h_token,
        ) != 0
        {
            let mut luid = Luid {
                low_part: 0,
                high_part: 0,
            };
            if LookupPrivilegeValueW(std::ptr::null(), priv_name.as_ptr(), &mut luid) != 0 {
                let tp = TokenPrivileges {
                    privilege_count: 1,
                    privileges: [LuidAndAttributes {
                        luid,
                        attributes: SE_PRIVILEGE_ENABLED,
                    }],
                };
                AdjustTokenPrivileges(
                    h_token,
                    0,
                    &tp,
                    size_of::<TokenPrivileges>() as u32,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                );
            }
            CloseHandle(h_token);
        }
    }
}

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

pub fn check_and_purge_standby_list(threshold_mb: u64, last_purge: &mut Instant) {
    if last_purge.elapsed() < Duration::from_secs(60) {
        return;
    }
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
                *last_purge = Instant::now();
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
    apply_gamma_ramp_force(color_cfg);
    Ok(())
}

fn compute_cpu_affinity_masks() -> (usize, usize) {
    let logical = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .min(64);
    let all_mask = if logical >= usize::BITS as usize {
        usize::MAX
    } else {
        (1usize << logical) - 1
    };
    let mut physical_mask = 0usize;
    for i in (0..logical).step_by(2) {
        physical_mask |= 1usize << i;
    }
    if physical_mask == 0 {
        physical_mask = 1;
    }
    (all_mask, physical_mask)
}

fn is_multithreaded_media_or_browser(h_proc: *mut c_void) -> bool {
    let mut buf = [0u16; 260];
    let mut len = buf.len() as u32;
    let ok = unsafe { QueryFullProcessImageNameW(h_proc, 0, buf.as_mut_ptr(), &mut len) };
    if ok == 0 || len == 0 {
        return false;
    }
    let full = String::from_utf16_lossy(&buf[..len as usize]).to_lowercase();
    let exe = full.rsplit('\\').next().unwrap_or("");
    MULTITHREADED_MEDIA_EXES.contains(&exe)
}

pub fn boost_foreground_process(all_cores_mask: usize, physical_core_mask: usize) {
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
        let h_proc = OpenProcess(
            PROCESS_SET_INFORMATION | PROCESS_QUERY_LIMITED_INFORMATION,
            0,
            pid,
        );
        if !h_proc.is_null() {
            let _ = SetPriorityClass(h_proc, HIGH_PRIORITY_CLASS);
            let target_mask = if is_multithreaded_media_or_browser(h_proc) {
                all_cores_mask
            } else {
                physical_core_mask
            };
            let _ = SetProcessAffinityMask(h_proc, target_mask);
            CloseHandle(h_proc);
        }
    }
}

pub fn run_daemon_loop(fallback_color: ColorConfig) {
    unsafe {
        let _ = SetPriorityClass(GetCurrentProcess(), ABOVE_NORMAL_PRIORITY_CLASS);
    }
    enable_standby_purge_privilege();
    lock_kernel_timer_05ms(true);

    let (all_cores_mask, physical_core_mask) = compute_cpu_affinity_masks();

    let hook_fg = unsafe {
        SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            std::ptr::null_mut(),
            Some(win_event_callback),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        )
    };

    let hook_focus = unsafe {
        SetWinEventHook(
            EVENT_OBJECT_FOCUS,
            EVENT_OBJECT_FOCUS,
            std::ptr::null_mut(),
            Some(win_event_callback),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        )
    };

    let mut last_purge = Instant::now() - Duration::from_secs(60);
    let mut last_fg_hwnd: isize = 0;
    let mut follow_up_pulse_due: Option<Instant> = None;

    let init_color = read_intel_cui_color_from_registry().unwrap_or(fallback_color);
    apply_gamma_ramp_force(&init_color);

    loop {
        unsafe {
            let mut msg: MSG = zeroed();
            while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        let cur_hwnd = unsafe { GetForegroundWindow() } as isize;
        if cur_hwnd != 0 && cur_hwnd != last_fg_hwnd {
            last_fg_hwnd = cur_hwnd;
            boost_foreground_process(all_cores_mask, physical_core_mask);
            UI_EVENT_TRIGGERED.store(true, Ordering::SeqCst);
        }

        if UI_EVENT_TRIGGERED.swap(false, Ordering::SeqCst) {
            let active_color = read_intel_cui_color_from_registry().unwrap_or(fallback_color);
            apply_gamma_ramp_force(&active_color);
            follow_up_pulse_due = Some(Instant::now() + Duration::from_millis(450));
        } else if let Some(due) = follow_up_pulse_due {
            if Instant::now() >= due {
                let active_color = read_intel_cui_color_from_registry().unwrap_or(fallback_color);
                apply_gamma_ramp_force(&active_color);
                follow_up_pulse_due = None;
            }
        }

        check_and_purge_standby_list(350, &mut last_purge);

        thread::sleep(Duration::from_millis(200));
    }

    #[allow(unreachable_code)]
    unsafe {
        if !hook_fg.is_null() {
            UnhookWinEvent(hook_fg);
        }
        if !hook_focus.is_null() {
            UnhookWinEvent(hook_focus);
        }
    }
}
