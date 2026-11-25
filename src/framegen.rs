use anyhow::{anyhow, Context, Result};
use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use windows::core::{Interface, PCSTR, PCWSTR};
use windows::Win32::Foundation::{HANDLE, HMODULE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Direct3D::Fxc::{D3DCompile, D3DCOMPILE_OPTIMIZATION_LEVEL3};
use windows::Win32::Graphics::Direct3D::{
    ID3DBlob, ID3DInclude, D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL_11_0,
    D3D_FEATURE_LEVEL_11_1, D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Buffer, ID3D11Device, ID3D11DeviceContext, ID3D11PixelShader,
    ID3D11RenderTargetView, ID3D11SamplerState, ID3D11ShaderResourceView, ID3D11Texture2D,
    ID3D11VertexShader, D3D11_BIND_CONSTANT_BUFFER, D3D11_BIND_RENDER_TARGET,
    D3D11_BIND_SHADER_RESOURCE, D3D11_BUFFER_DESC, D3D11_COMPARISON_NEVER,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_FILTER_MIN_MAG_MIP_LINEAR, D3D11_SAMPLER_DESC,
    D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_TEXTURE_ADDRESS_CLAMP, D3D11_USAGE_DEFAULT,
    D3D11_VIEWPORT,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_R16G16_FLOAT, DXGI_FORMAT_R16_FLOAT,
    DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIAdapter, IDXGIAdapter1, IDXGIFactory2, IDXGIOutput1,
    IDXGIOutputDuplication, IDXGIResource, IDXGISwapChain1, IDXGISwapChain2,
    DXGI_ALPHA_MODE_IGNORE, DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO, DXGI_SCALING_STRETCH,
    DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT,
    DXGI_SWAP_EFFECT_FLIP_DISCARD, DXGI_USAGE_RENDER_TARGET_OUTPUT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::WaitForSingleObjectEx;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL, VK_MENU};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetCursorPos,
    GetSystemMetrics, PeekMessageW, RegisterClassW, SetWindowDisplayAffinity, ShowWindow,
    TranslateMessage, CS_HREDRAW, CS_VREDRAW, MSG, PM_REMOVE, SM_CXSCREEN, SM_CYSCREEN, SW_HIDE,
    SW_SHOWNOACTIVATE, WDA_EXCLUDEFROMCAPTURE, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

use crate::daemon::lock_kernel_timer_05ms;

const SHADER_SRC: &str = include_str!("shaders.hlsl");

#[derive(Clone, Copy, Debug)]
pub enum FrameGenMode {
    PyramidOpticalFlow = 0,
    UltraLiteLucasKanade = 1,
    AdaptiveVideoSmoother = 2,
}

#[derive(Clone, Copy, Debug)]
pub enum LatencyMode {
    ZeroLagExtrapolation,
    ClassicInterpolation,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct FrameGenCBuffer {
    texel_size: [f32; 2],
    flow_texel_size: [f32; 2],
    mouse_bias_uv: [f32; 2],
    alpha: f32,
    mode: f32,
    zero_lag_extrap: f32,
    hud_threshold: f32,
    scene_cut_thresh: f32,
    _pad: f32,
}

struct RenderTargetTex {
    tex: ID3D11Texture2D,
    srv: ID3D11ShaderResourceView,
    rtv: ID3D11RenderTargetView,
    width: u32,
    height: u32,
}

static OVERLAY_ACTIVE: AtomicBool = AtomicBool::new(true);

unsafe extern "system" fn overlay_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Strictly locates an Intel HD Graphics 4400 Family (Haswell) adapter (`VendorId == 0x8086`,
/// DeviceId in `0x0400..=0x043F`, `0x0A00..=0x0A3F`, or `0x0D00..=0x0D3F`) so Frame Gen
/// NEVER steals load from a discrete GPU and never attempts to run on unsupported Ironlake.
fn find_intel_hd4400_adapter(factory: &IDXGIFactory2) -> Result<(IDXGIAdapter1, String)> {
    let mut i = 0;
    loop {
        let adapter = match unsafe { factory.EnumAdapters1(i) } {
            Ok(a) => a,
            Err(_) => break,
        };
        let desc = unsafe { adapter.GetDesc1()? };
        let name_len = desc
            .Description
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(desc.Description.len());
        let name = String::from_utf16_lossy(&desc.Description[..name_len]);

        if desc.VendorId == 0x8086 {
            let dev = desc.DeviceId;
            let is_haswell = (0x0400..=0x043F).contains(&dev)
                || (0x0A00..=0x0A3F).contains(&dev)
                || (0x0D00..=0x0D3F).contains(&dev);
            if is_haswell {
                return Ok((adapter, name));
            }
        }
        i += 1;
    }

    Err(anyhow!(
        "Intel HD Graphics 4400 Family (Haswell) DXGI adapter not found! \
        Dual-GPU Frame Gen is locked strictly to Intel HD 4400 (DEV_04xx / DEV_0Axx / DEV_0Dxx)."
    ))
}

fn compile_shader(entry: &str, target: &str) -> Result<Vec<u8>> {
    let entry_c = std::ffi::CString::new(entry)?;
    let target_c = std::ffi::CString::new(target)?;
    let mut code: Option<ID3DBlob> = None;
    let mut err: Option<ID3DBlob> = None;

    let res = unsafe {
        D3DCompile(
            SHADER_SRC.as_ptr() as *const c_void,
            SHADER_SRC.len(),
            PCSTR::null(),
            None,
            None::<&ID3DInclude>,
            PCSTR(entry_c.as_ptr() as *const u8),
            PCSTR(target_c.as_ptr() as *const u8),
            D3DCOMPILE_OPTIMIZATION_LEVEL3,
            0,
            &mut code,
            Some(&mut err),
        )
    };

    if let Err(e) = res {
        let msg = if let Some(eb) = err {
            unsafe {
                let slice = std::slice::from_raw_parts(
                    eb.GetBufferPointer() as *const u8,
                    eb.GetBufferSize(),
                );
                String::from_utf8_lossy(slice).to_string()
            }
        } else {
            e.to_string()
        };
        return Err(anyhow!("HLSL compile error ({}): {}", entry, msg));
    }

    let blob = code.ok_or_else(|| anyhow!("D3DCompile returned empty bytecode"))?;
    let bytes = unsafe {
        std::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize())
            .to_vec()
    };
    Ok(bytes)
}

fn create_rt_texture(
    device: &ID3D11Device,
    width: u32,
    height: u32,
    format: DXGI_FORMAT,
) -> Result<RenderTargetTex> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: width.max(1),
        Height: height.max(1),
        MipLevels: 1,
        ArraySize: 1,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: (D3D11_BIND_SHADER_RESOURCE.0 | D3D11_BIND_RENDER_TARGET.0) as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };

    let mut tex: Option<ID3D11Texture2D> = None;
    unsafe { device.CreateTexture2D(&desc, None, Some(&mut tex))? };
    let tex = tex.unwrap();

    let mut srv: Option<ID3D11ShaderResourceView> = None;
    unsafe { device.CreateShaderResourceView(&tex, None, Some(&mut srv))? };

    let mut rtv: Option<ID3D11RenderTargetView> = None;
    unsafe { device.CreateRenderTargetView(&tex, None, Some(&mut rtv))? };

    Ok(RenderTargetTex {
        tex,
        srv: srv.unwrap(),
        rtv: rtv.unwrap(),
        width: desc.Width,
        height: desc.Height,
    })
}

fn create_overlay_window(width: i32, height: i32) -> Result<HWND> {
    unsafe {
        let hinstance = GetModuleHandleW(PCWSTR::null())?;
        let class_name = to_wide("KBridgeFrameGenOverlay");

        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(overlay_wnd_proc),
            hInstance: hinstance.into(),
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..zeroed()
        };
        RegisterClassW(&wc);

        let ex_style = WS_EX_TOPMOST
            | WS_EX_TRANSPARENT
            | WS_EX_LAYERED
            | WS_EX_NOACTIVATE
            | WS_EX_TOOLWINDOW;

        let hwnd = CreateWindowExW(
            ex_style,
            PCWSTR(class_name.as_ptr()),
            PCWSTR(to_wide("KBridge ZeroLag-X").as_ptr()),
            WS_POPUP,
            0,
            0,
            width,
            height,
            HWND::default(),
            None,
            hinstance,
            None,
        )?;

        // CRITICAL: Exclude our own overlay from DXGI Desktop Duplication so zero feedback loop occurs
        SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE)?;
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        Ok(hwnd)
    }
}

fn run_pass(
    ctx: &ID3D11DeviceContext,
    vs: &ID3D11VertexShader,
    ps: &ID3D11PixelShader,
    rtv: &ID3D11RenderTargetView,
    srvs: &[Option<ID3D11ShaderResourceView>],
    sampler: &ID3D11SamplerState,
    cb: &ID3D11Buffer,
    w: u32,
    h: u32,
) {
    let vp = D3D11_VIEWPORT {
        TopLeftX: 0.0,
        TopLeftY: 0.0,
        Width: w as f32,
        Height: h as f32,
        MinDepth: 0.0,
        MaxDepth: 1.0,
    };
    unsafe {
        ctx.OMSetRenderTargets(Some(&[Some(rtv.clone())]), None);
        ctx.RSSetViewports(Some(&[vp]));
        ctx.VSSetShader(vs, None);
        ctx.PSSetShader(ps, None);
        ctx.PSSetSamplers(0, Some(&[Some(sampler.clone())]));
        ctx.PSSetConstantBuffers(0, Some(&[Some(cb.clone())]));
        ctx.PSSetShaderResources(0, Some(srvs));
        ctx.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        ctx.Draw(3, 0);
        // Unbind SRVs so textures can be reused as RTVs cleanly next pass without D3D11 hazard warnings
        let nulls: [Option<ID3D11ShaderResourceView>; 3] = [None, None, None];
        ctx.PSSetShaderResources(0, Some(&nulls));
    }
}

fn update_cb(ctx: &ID3D11DeviceContext, cb: &ID3D11Buffer, data: &FrameGenCBuffer) {
    unsafe {
        ctx.UpdateSubresource(cb, 0, None, data as *const _ as *const c_void, 0, 0);
    }
}

fn high_precision_wait(target: Instant) {
    let now = Instant::now();
    if target <= now {
        return;
    }
    let rem = target - now;
    if rem > Duration::from_micros(1800) {
        thread::sleep(rem - Duration::from_micros(1500));
    }
    while Instant::now() < target {
        std::hint::spin_loop();
    }
}

fn get_cursor_pos() -> (i32, i32) {
    let mut pt = POINT { x: 0, y: 0 };
    unsafe {
        let _ = GetCursorPos(&mut pt);
    }
    (pt.x, pt.y)
}

fn is_hotkey_pressed(vk_letter: i32) -> bool {
    unsafe {
        (GetAsyncKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0
            && (GetAsyncKeyState(VK_MENU.0 as i32) as u16 & 0x8000) != 0
            && (GetAsyncKeyState(vk_letter) as u16 & 0x8000) != 0
    }
}

pub fn run_hd4400_framegen(mode: FrameGenMode, latency: LatencyMode) -> Result<()> {
    println!("\n[*] Initializing KBridge Dual-GPU Frame Gen on Intel HD Graphics 4400...");
    let factory: IDXGIFactory2 = unsafe { CreateDXGIFactory1()? };
    let (adapter1, adapter_name) = find_intel_hd4400_adapter(&factory)?;
    println!("[+] Locked Offload Adapter: {} (0% load on dGPU)", adapter_name);

    let adapter_base: IDXGIAdapter = adapter1.cast()?;
    let feature_levels = [D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0];
    let mut device: Option<ID3D11Device> = None;
    let mut context: Option<ID3D11DeviceContext> = None;

    unsafe {
        D3D11CreateDevice(
            &adapter_base,
            D3D_DRIVER_TYPE_UNKNOWN,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&feature_levels),
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )
        .context("D3D11CreateDevice failed on Intel HD 4400")?;
    }
    let device = device.unwrap();
    let ctx = context.unwrap();

    // Acquire DXGI Desktop Duplication on Output 0 wired to Intel HD 4400
    let output0 = unsafe {
        adapter1
            .EnumOutputs(0)
            .context("No active display output wired to Intel HD 4400")?
    };
    let output1: IDXGIOutput1 = output0.cast()?;
    let duplication: IDXGIOutputDuplication = unsafe {
        output1
            .DuplicateOutput(&device)
            .context("DuplicateOutput failed (ensure game is Borderless Windowed and display is on HD 4400)")?
    };

    let width = unsafe { GetSystemMetrics(SM_CXSCREEN) } as u32;
    let height = unsafe { GetSystemMetrics(SM_CYSCREEN) } as u32;
    let hwnd = create_overlay_window(width as i32, height as i32)?;

    // Waitable 1-Frame Flip-Discard SwapChain for absolute minimum presentation latency
    let swap_desc = DXGI_SWAP_CHAIN_DESC1 {
        Width: width,
        Height: height,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        Stereo: false.into(),
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: 2,
        Scaling: DXGI_SCALING_STRETCH,
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
        AlphaMode: DXGI_ALPHA_MODE_IGNORE,
        Flags: DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT.0 as u32,
    };

    let swap_chain1: IDXGISwapChain1 = unsafe {
        factory.CreateSwapChainForHwnd(&device, hwnd, &swap_desc, None, None)?
    };
    let swap_chain2: IDXGISwapChain2 = swap_chain1.cast()?;
    unsafe { swap_chain2.SetMaximumFrameLatency(1)? };
    let waitable_obj: HANDLE = unsafe { swap_chain2.GetFrameLatencyWaitableObject() };

    let back_buffer: ID3D11Texture2D = unsafe { swap_chain1.GetBuffer(0)? };
    let mut back_rtv: Option<ID3D11RenderTargetView> = None;
    unsafe { device.CreateRenderTargetView(&back_buffer, None, Some(&mut back_rtv))? };
    let back_rtv = back_rtv.unwrap();

    // Compile HLSL Shaders on Intel HD 4400 (SM 4.0 / 5.0)
    println!("[*] Compiling KBridge HLSL Optical Flow & ZeroLag-X Synthesis shaders...");
    let vs_bytes = compile_shader("VS_Fullscreen", "vs_4_0")?;
    let ps_luma_b = compile_shader("PS_LumaDownsample", "ps_4_0")?;
    let ps_coarse_b = compile_shader("PS_PyramidCoarse", "ps_4_0")?;
    let ps_fine_b = compile_shader("PS_PyramidFine", "ps_4_0")?;
    let ps_smooth_b = compile_shader("PS_SmoothFlow", "ps_4_0")?;
    let ps_lk_b = compile_shader("PS_LucasKanade", "ps_4_0")?;
    let ps_pass_b = compile_shader("PS_Passthrough", "ps_4_0")?;
    let ps_synth_b = compile_shader("PS_Synthesize", "ps_4_0")?;

    let mut vs: Option<ID3D11VertexShader> = None;
    let mut ps_luma: Option<ID3D11PixelShader> = None;
    let mut ps_coarse: Option<ID3D11PixelShader> = None;
    let mut ps_fine: Option<ID3D11PixelShader> = None;
    let mut ps_smooth: Option<ID3D11PixelShader> = None;
    let mut ps_lk: Option<ID3D11PixelShader> = None;
    let mut ps_pass: Option<ID3D11PixelShader> = None;
    let mut ps_synth: Option<ID3D11PixelShader> = None;

    unsafe {
        device.CreateVertexShader(&vs_bytes, None, Some(&mut vs))?;
        device.CreatePixelShader(&ps_luma_b, None, Some(&mut ps_luma))?;
        device.CreatePixelShader(&ps_coarse_b, None, Some(&mut ps_coarse))?;
        device.CreatePixelShader(&ps_fine_b, None, Some(&mut ps_fine))?;
        device.CreatePixelShader(&ps_smooth_b, None, Some(&mut ps_smooth))?;
        device.CreatePixelShader(&ps_lk_b, None, Some(&mut ps_lk))?;
        device.CreatePixelShader(&ps_pass_b, None, Some(&mut ps_pass))?;
        device.CreatePixelShader(&ps_synth_b, None, Some(&mut ps_synth))?;
    }
    let vs = vs.unwrap();
    let ps_luma = ps_luma.unwrap();
    let ps_coarse = ps_coarse.unwrap();
    let ps_fine = ps_fine.unwrap();
    let ps_smooth = ps_smooth.unwrap();
    let ps_lk = ps_lk.unwrap();
    let ps_pass = ps_pass.unwrap();
    let ps_synth = ps_synth.unwrap();

    // Linear clamp sampler & Constant Buffer
    let samp_desc = D3D11_SAMPLER_DESC {
        Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
        AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
        AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
        AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
        ComparisonFunc: D3D11_COMPARISON_NEVER,
        MinLOD: 0.0,
        MaxLOD: f32::MAX,
        ..unsafe { zeroed() }
    };
    let mut sampler: Option<ID3D11SamplerState> = None;
    unsafe { device.CreateSamplerState(&samp_desc, Some(&mut sampler))? };
    let sampler = sampler.unwrap();

    let cb_desc = D3D11_BUFFER_DESC {
        ByteWidth: size_of::<FrameGenCBuffer>() as u32,
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
        StructureByteStride: 0,
    };
    let mut cb: Option<ID3D11Buffer> = None;
    unsafe { device.CreateBuffer(&cb_desc, None, Some(&mut cb))? };
    let cb = cb.unwrap();

    // Allocate Full-Res Ring Buffers + 1/4 & 1/8 Pyramid Buffers in HD 4400 VRAM
    let w4 = (width / 4).max(16);
    let h4 = (height / 4).max(16);
    let w8 = (width / 8).max(8);
    let h8 = (height / 8).max(8);

    let full_prev = create_rt_texture(&device, width, height, DXGI_FORMAT_B8G8R8A8_UNORM)?;
    let full_curr = create_rt_texture(&device, width, height, DXGI_FORMAT_B8G8R8A8_UNORM)?;
    let luma4_prev = create_rt_texture(&device, w4, h4, DXGI_FORMAT_R16_FLOAT)?;
    let luma4_curr = create_rt_texture(&device, w4, h4, DXGI_FORMAT_R16_FLOAT)?;
    let luma8_prev = create_rt_texture(&device, w8, h8, DXGI_FORMAT_R16_FLOAT)?;
    let luma8_curr = create_rt_texture(&device, w8, h8, DXGI_FORMAT_R16_FLOAT)?;
    let flow_coarse = create_rt_texture(&device, w8, h8, DXGI_FORMAT_R16G16_FLOAT)?;
    let flow_fine = create_rt_texture(&device, w4, h4, DXGI_FORMAT_R16G16_FLOAT)?;
    let flow_smooth = create_rt_texture(&device, w4, h4, DXGI_FORMAT_R16G16_FLOAT)?;

    lock_kernel_timer_05ms(true);
    OVERLAY_ACTIVE.store(true, Ordering::SeqCst);

    println!("[+] KBridge HD 4400 Frame Gen ACTIVE ({:?} | {:?})!", mode, latency);
    println!("[i] Ensure your game is in Borderless Windowed mode.");
    println!("[i] Hotkeys: [Ctrl + Alt + F] Toggle Overlay | [Ctrl + Alt + Q] Quit Frame Gen");

    let mut has_prev = false;
    let mut last_real_time = Instant::now();
    let mut avg_frame_dt_ms: f64 = 33.33; // Default 30fps base estimate
    let mut last_toggle_time = Instant::now() - Duration::from_secs(1);

    let mut cb_data = FrameGenCBuffer {
        texel_size: [1.0 / width as f32, 1.0 / height as f32],
        flow_texel_size: [1.0 / w4 as f32, 1.0 / h4 as f32],
        mouse_bias_uv: [0.0, 0.0],
        alpha: 0.5,
        mode: (mode as u32) as f32,
        zero_lag_extrap: matches!(latency, LatencyMode::ZeroLagExtrapolation) as u32 as f32,
        hud_threshold: 0.015,
        scene_cut_thresh: 0.28,
        _pad: 0.0,
    };

    loop {
        // Pump overlay window messages
        unsafe {
            let mut msg: MSG = zeroed();
            while PeekMessageW(&mut msg, hwnd, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        // Hotkey: Ctrl + Alt + Q -> Exit
        if is_hotkey_pressed(b'Q' as i32) {
            println!("[*] Stopping HD 4400 Frame Gen...");
            break;
        }

        // Hotkey: Ctrl + Alt + F -> Toggle Overlay
        if is_hotkey_pressed(b'F' as i32) && last_toggle_time.elapsed() > Duration::from_millis(400) {
            let now_on = !OVERLAY_ACTIVE.load(Ordering::SeqCst);
            OVERLAY_ACTIVE.store(now_on, Ordering::SeqCst);
            unsafe {
                let _ = ShowWindow(hwnd, if now_on { SW_SHOWNOACTIVATE } else { SW_HIDE });
            }
            println!("[*] Frame Gen Overlay: {}", if now_on { "ON" } else { "PAUSED" });
            last_toggle_time = Instant::now();
        }

        if !OVERLAY_ACTIVE.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(50));
            continue;
        }

        // 1. Capture next real frame from DXGI Desktop Duplication on Intel HD 4400
        let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut desktop_res: Option<IDXGIResource> = None;
        let acq = unsafe { duplication.AcquireNextFrame(16, &mut frame_info, &mut desktop_res) };

        match acq {
            Ok(()) => {
                let capture_time = Instant::now();
                let (mx0, my0) = get_cursor_pos();

                let dt_ms = (capture_time - last_real_time).as_secs_f64() * 1000.0;
                if dt_ms > 5.0 && dt_ms < 100.0 {
                    avg_frame_dt_ms = avg_frame_dt_ms * 0.8 + dt_ms * 0.2;
                }
                last_real_time = capture_time;

                if let Some(res) = desktop_res {
                    let src_tex: ID3D11Texture2D = res.cast()?;
                    unsafe {
                        // Shift previous -> prev, copy fresh -> curr
                        if has_prev {
                            ctx.CopyResource(&full_prev.tex, &full_curr.tex);
                            ctx.CopyResource(&luma4_prev.tex, &luma4_curr.tex);
                            ctx.CopyResource(&luma8_prev.tex, &luma8_curr.tex);
                        }
                        ctx.CopyResource(&full_curr.tex, &src_tex);
                    }
                }
                let _ = unsafe { duplication.ReleaseFrame() };

                // Downsample fresh frame to 1/4 and 1/8 Luma
                update_cb(&ctx, &cb, &cb_data);
                run_pass(
                    &ctx,
                    &vs,
                    &ps_luma,
                    &luma4_curr.rtv,
                    &[None, Some(full_curr.srv.clone())],
                    &sampler,
                    &cb,
                    w4,
                    h4,
                );
                if matches!(mode, FrameGenMode::PyramidOpticalFlow) {
                    run_pass(
                        &ctx,
                        &vs,
                        &ps_luma,
                        &luma8_curr.rtv,
                        &[None, Some(luma4_curr.srv.clone())],
                        &sampler,
                        &cb,
                        w8,
                        h8,
                    );
                }

                if !has_prev {
                    has_prev = true;
                    continue;
                }

                // 2. If ZeroLag-X Extrapolation: present REAL Frame[N] IMMEDIATELY (0-Frame Hold!)
                if matches!(latency, LatencyMode::ZeroLagExtrapolation) {
                    unsafe {
                        WaitForSingleObjectEx(waitable_obj, 50, false);
                    }
                    run_pass(
                        &ctx,
                        &vs,
                        &ps_pass,
                        &back_rtv,
                        &[None, Some(full_curr.srv.clone())],
                        &sampler,
                        &cb,
                        width,
                        height,
                    );
                    let _ = unsafe { swap_chain1.Present(0, 0) };
                }

                // 3. Run Optical Flow on Intel HD 4400
                let final_flow_srv = match mode {
                    FrameGenMode::PyramidOpticalFlow => {
                        cb_data.flow_texel_size = [1.0 / w8 as f32, 1.0 / h8 as f32];
                        update_cb(&ctx, &cb, &cb_data);
                        run_pass(
                            &ctx,
                            &vs,
                            &ps_coarse,
                            &flow_coarse.rtv,
                            &[Some(luma8_prev.srv.clone()), Some(luma8_curr.srv.clone())],
                            &sampler,
                            &cb,
                            w8,
                            h8,
                        );

                        cb_data.flow_texel_size = [1.0 / w4 as f32, 1.0 / h4 as f32];
                        update_cb(&ctx, &cb, &cb_data);
                        run_pass(
                            &ctx,
                            &vs,
                            &ps_fine,
                            &flow_fine.rtv,
                            &[
                                Some(luma4_prev.srv.clone()),
                                Some(luma4_curr.srv.clone()),
                                Some(flow_coarse.srv.clone()),
                            ],
                            &sampler,
                            &cb,
                            w4,
                            h4,
                        );

                        run_pass(
                            &ctx,
                            &vs,
                            &ps_smooth,
                            &flow_smooth.rtv,
                            &[None, None, Some(flow_fine.srv.clone())],
                            &sampler,
                            &cb,
                            w4,
                            h4,
                        );
                        flow_smooth.srv.clone()
                    }
                    FrameGenMode::UltraLiteLucasKanade | FrameGenMode::AdaptiveVideoSmoother => {
                        cb_data.flow_texel_size = [1.0 / w4 as f32, 1.0 / h4 as f32];
                        update_cb(&ctx, &cb, &cb_data);
                        run_pass(
                            &ctx,
                            &vs,
                            &ps_lk,
                            &flow_fine.rtv,
                            &[Some(luma4_prev.srv.clone()), Some(luma4_curr.srv.clone())],
                            &sampler,
                            &cb,
                            w4,
                            h4,
                        );
                        flow_fine.srv.clone()
                    }
                };

                // 4. JIT Half-Period Pacing + Live Mouse-Delta Camera-Vector Injection (ZeroLag-X)
                let half_period = Duration::from_secs_f64((avg_frame_dt_ms * 0.48) / 1000.0);
                let target_gen_time = capture_time + half_period;

                if matches!(latency, LatencyMode::ZeroLagExtrapolation) {
                    high_precision_wait(target_gen_time);
                    let (mx1, my1) = get_cursor_pos();
                    let dx_uv = ((mx1 - mx0) as f32 / width as f32).clamp(-0.025, 0.025) * 0.35;
                    let dy_uv = ((my1 - my0) as f32 / height as f32).clamp(-0.025, 0.025) * 0.35;
                    cb_data.mouse_bias_uv = [-dx_uv, -dy_uv];
                } else {
                    cb_data.mouse_bias_uv = [0.0, 0.0];
                }

                update_cb(&ctx, &cb, &cb_data);

                // Synthesize and present Generated Frame
                unsafe {
                    WaitForSingleObjectEx(waitable_obj, 50, false);
                }
                run_pass(
                    &ctx,
                    &vs,
                    &ps_synth,
                    &back_rtv,
                    &[
                        Some(full_prev.srv.clone()),
                        Some(full_curr.srv.clone()),
                        Some(final_flow_srv),
                    ],
                    &sampler,
                    &cb,
                    width,
                    height,
                );
                let _ = unsafe { swap_chain1.Present(0, 0) };

                // If Classic Interpolation: wait half-period and then present Real Frame[N]
                if matches!(latency, LatencyMode::ClassicInterpolation) {
                    high_precision_wait(target_gen_time);
                    unsafe {
                        WaitForSingleObjectEx(waitable_obj, 50, false);
                    }
                    run_pass(
                        &ctx,
                        &vs,
                        &ps_pass,
                        &back_rtv,
                        &[None, Some(full_curr.srv.clone())],
                        &sampler,
                        &cb,
                        width,
                        height,
                    );
                    let _ = unsafe { swap_chain1.Present(0, 0) };
                }
            }
            Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => {
                // Static desktop/video pause: no new frame from dGPU yet
            }
            Err(e) => {
                println!("[!] DXGI Desktop Duplication lost ({}), exiting Frame Gen.", e);
                break;
            }
        }
    }

    unsafe {
        let _ = DestroyWindow(hwnd);
    }
    Ok(())
}
