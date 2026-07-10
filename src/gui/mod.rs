use std::ffi::CString;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use windows::core::{w, Interface, PCWSTR};
use windows::Win32::{
    Foundation::*,
    Graphics::{
        Direct3D::D3D_DRIVER_TYPE_HARDWARE,
        Direct3D11::*,
        Dxgi::{Common::*, *},
        Gdi::*,
    },
    System::LibraryLoader::GetModuleHandleW,
    UI::WindowsAndMessaging::*,
};

mod components;
mod imgui;

use imgui::imgui_ffi::*;
use components::constants::*;

#[link(name = "dwmapi")]
extern "system" {
    fn DwmSetWindowAttribute(
        hwnd: isize,
        attr: u32,
        attr_value: *const std::ffi::c_void,
        attr_size: u32,
    ) -> i32;
}

// ── Global state (same pattern as NTSM agent) ───────────────────────────────
static mut G_RTV: Option<ID3D11RenderTargetView> = None;
static mut G_DEVICE: Option<ID3D11Device> = None;
static mut G_CONTEXT: Option<ID3D11DeviceContext> = None;
static mut G_SWAPCHAIN: Option<IDXGISwapChain> = None;
static mut G_RUNNING: bool = true;

const WINDOW_CLASS: PCWSTR = w!("RtspProxyLauncherWnd");
const WINDOW_W: i32 = 500;
const WINDOW_H: i32 = 340;   // increased height so all elements fit comfortably

struct AppData {
    child: Option<Child>,
    host: String,
    port: String,
    status: String,
}
static mut APP_DATA: Option<Mutex<AppData>> = None;

// ── Window procedure ────────────────────────────────────────────────────────
unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
) -> LRESULT {
    if c_ImGui_ImplWin32_WndProcHandler(hwnd.0 as isize, msg, wp.0, lp.0) != 0 {
        return LRESULT(1);
    }

    match msg {
        WM_SIZE => {
            let w = (lp.0 & 0xFFFF) as u32;
            let h = ((lp.0 >> 16) & 0xFFFF) as u32;
            if w > 0 && h > 0 {
                if let (Some(ctx), Some(sc)) = (G_CONTEXT.as_ref(), G_SWAPCHAIN.as_ref()) {
                    ctx.OMSetRenderTargets(None, None);
                    G_RTV = None;
                    let _ = sc.ResizeBuffers(0, w, h, DXGI_FORMAT_UNKNOWN, DXGI_SWAP_CHAIN_FLAG(0));
                    G_RTV = create_rtv();
                }
            }
            LRESULT(0)
        }
        WM_CLOSE | WM_DESTROY => {
            G_RUNNING = false;
            PostQuitMessage(0);
            LRESULT(0)
        }
        WM_SYSCOMMAND => {
            if (wp.0 & 0xFFF0) == SC_KEYMENU as usize {
                return LRESULT(0);
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

// ── Helper to recreate render target view after resize ──────────────────────
unsafe fn create_rtv() -> Option<ID3D11RenderTargetView> {
    let sc = G_SWAPCHAIN.as_ref()?;
    let dev = G_DEVICE.as_ref()?;
    let back_buffer: ID3D11Texture2D = sc.GetBuffer(0).ok()?;
    let mut rtv: Option<ID3D11RenderTargetView> = None;
    dev.CreateRenderTargetView(&back_buffer, None, Some(&mut rtv)).ok()?;
    rtv
}

// ── Render one frame (identical style to NTSM agent) ────────────────────────
unsafe fn render_frame() {
    let mut app = APP_DATA.as_ref().unwrap().lock().unwrap();

    let dw = ig_get_display_w();
    let dh = ig_get_display_h();

    // Push style settings
    igPushStyleVar_Float(SV_WINDOW_ROUNDING, 0.0);
    igPushStyleVar_Float(SV_WINDOW_BORDERSIZE, 0.0);
    igPushStyleColor_Vec4(IM_COL_WINDOW_BG, 0.0, 0.0, 0.0, 0.0);
    igPushStyleColor_Vec4(IM_COL_BORDER, 0.0, 0.0, 0.0, 0.0);
    igPushStyleColor_Vec4(IM_COL_FRAME_BG, 30.0 / 255.0, 30.0 / 255.0, 35.0 / 255.0, 1.0);
    igPushStyleColor_Vec4(IM_COL_FRAME_BG_HOV, 38.0 / 255.0, 38.0 / 255.0, 44.0 / 255.0, 1.0);
    igPushStyleColor_Vec4(IM_COL_FRAME_BG_ACT, 44.0 / 255.0, 44.0 / 255.0, 52.0 / 255.0, 1.0);

    // Full‑screen transparent background
    igSetNextWindowPos(0.0, 0.0, 8, 0.0, 0.0);
    igSetNextWindowSize(dw, dh, 8);
    let bg_flags = (1 << 0) | (1 << 1) | (1 << 2) | (1 << 3)
                 | (1 << 4) | (1 << 5) | (1 << 7) | (1 << 20);
    let bg_cstr = CString::new("##bg").unwrap();
    igBegin(bg_cstr.as_ptr(), std::ptr::null_mut(), bg_flags);
    igEnd();

    let dl = igGetWindowDrawList();

    // ── Host field ──────────────────────────────────────────────────────────
    {
        let label = CString::new("Host").unwrap();
        let mut lw = 0.0f32;
        let mut lh = 0.0f32;
        igCalcTextSize(&mut lw, &mut lh, label.as_ptr());

        let row_y = 40.0;
        igSetCursorPos(SIDE_MARGIN, row_y + 14.0);
        igTextColored(MUTED.0, MUTED.1, MUTED.2, MUTED.3, label.as_ptr());

        igSetCursorPos(LABEL_X, row_y);
        igPushStyleVar_Float(SV_FRAME_ROUNDING, 7.0);

        let mut host_buf = [0u8; 128];
        for (i, &b) in app.host.as_bytes().iter().enumerate() {
            host_buf[i] = b;
        }
        let field_w = dw - LABEL_X - SIDE_MARGIN;
        let changed = components::text_field::render_text_field(
            "host",
            host_buf.as_mut_ptr() as *mut i8,
            host_buf.len(),
            field_w,
        );
        igPopStyleVar(1);
        if changed {
            app.host = String::from_utf8_lossy(&host_buf)
                .trim_end_matches('\0')
                .to_string();
        }
    }

    // ── Port field ──────────────────────────────────────────────────────────
    {
        let label = CString::new("Port").unwrap();
        let mut lw = 0.0f32;
        let mut lh = 0.0f32;
        igCalcTextSize(&mut lw, &mut lh, label.as_ptr());

        let row_y = 110.0;
        igSetCursorPos(SIDE_MARGIN, row_y + 14.0);
        igTextColored(MUTED.0, MUTED.1, MUTED.2, MUTED.3, label.as_ptr());

        igSetCursorPos(LABEL_X, row_y);
        igPushStyleVar_Float(SV_FRAME_ROUNDING, 7.0);

        let mut port_buf = [0u8; 32];
        for (i, &b) in app.port.as_bytes().iter().enumerate() {
            port_buf[i] = b;
        }
        let field_w = dw - LABEL_X - SIDE_MARGIN;
        let changed = components::text_field::render_text_field(
            "port",
            port_buf.as_mut_ptr() as *mut i8,
            port_buf.len(),
            field_w,
        );
        igPopStyleVar(1);
        if changed {
            app.port = String::from_utf8_lossy(&port_buf)
                .trim_end_matches('\0')
                .to_string();
        }
    }

    // ── Start / Stop button ────────────────────────────────────────────────
    const BTN_W: f32 = 220.0;
    const BTN_H: f32 = 42.0;
    let btn_x = (dw - BTN_W) * 0.5;
    let btn_y = dh - BTN_H - 30.0;
    igSetCursorPos(btn_x, btn_y);

    if app.child.is_none() {
        if components::gradient_button::gradient_button(dl, "Start Server", BTN_W, BTN_H) {
            start_server_internal(&mut app);
        }
    } else {
        if components::gradient_button::gradient_button(dl, "Stop Server", BTN_W, BTN_H) {
            stop_server_internal(&mut app);
        }
    }

    // ── Status line ────────────────────────────────────────────────────────
    {
        let text = CString::new(format!("Status: {}", app.status)).unwrap();
        let mut tw = 0.0f32;
        let mut th = 0.0f32;
        igCalcTextSize(&mut tw, &mut th, text.as_ptr());
        igSetCursorPos((dw - tw) * 0.5, btn_y - th - 10.0);
        igTextColored(0.5, 0.5, 0.5, 1.0, text.as_ptr());
    }

    igPopStyleColor(5);
    igPopStyleVar(2);
}

// ── Server process management (unchanged) ──────────────────────────────────
fn start_server_internal(app: &mut AppData) {
    if app.child.is_some() { return; }
    let exe = std::env::current_exe().expect("cannot get own exe path");
    let mut cmd = Command::new(exe);
    cmd.args(["--server", "--host", &app.host, "--port", &app.port])
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    match cmd.spawn() {
        Ok(child) => {
            app.child = Some(child);
            app.status = format!("Running on http://{}:{}", app.host, app.port);
        }
        Err(e) => app.status = format!("Error: {}", e),
    }
}

fn stop_server_internal(app: &mut AppData) {
    if let Some(mut child) = app.child.take() {
        let _ = child.kill();
        let _ = child.wait();
        app.status = "Stopped".to_string();
    }
}

fn poll_child(app: &mut AppData) {
    if let Some(ref mut child) = app.child {
        match child.try_wait() {
            Ok(Some(exit)) => {
                app.child = None;
                app.status = format!("Server exited (code {})", exit);
            }
            Ok(None) => {
                app.status = format!("Running on http://{}:{}", app.host, app.port);
            }
            Err(e) => {
                app.child = None;
                app.status = format!("Error: {}", e);
            }
        }
    }
}

// ── Main entry point ───────────────────────────────────────────────────────
pub fn run_gui() {
    unsafe {
        let instance = GetModuleHandleW(None).unwrap();
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap(),
            lpszClassName: WINDOW_CLASS,
            ..Default::default()
        };
        RegisterClassExW(&wc);

        let hwnd = CreateWindowExW(
            Default::default(),
            WINDOW_CLASS,
            w!("RTSP Proxy"),
            WS_OVERLAPPEDWINDOW & !WS_THICKFRAME & !WS_MAXIMIZEBOX,
            CW_USEDEFAULT, CW_USEDEFAULT,
            WINDOW_W, WINDOW_H,
            None, None, instance, None,
        )
        .unwrap();

        // Dark title bar (same as NTSM agent)
        let dark: BOOL = BOOL(1);
        DwmSetWindowAttribute(
            hwnd.0 as isize,
            20, // DWMWA_USE_IMMERSIVE_DARK_MODE
            &dark as *const BOOL as *const std::ffi::c_void,
            std::mem::size_of::<BOOL>() as u32,
        );
        let title_color: u32 = 0x00191918;   // BGR #181819
        DwmSetWindowAttribute(
            hwnd.0 as isize,
            35, // DWMWA_CAPTION_COLOR
            &title_color as *const u32 as *const std::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        );

        // Center on screen
        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let screen_h = GetSystemMetrics(SM_CYSCREEN);
        SetWindowPos(hwnd, HWND_TOP, (screen_w - WINDOW_W) / 2, (screen_h - WINDOW_H) / 2, 0, 0, SWP_NOSIZE);

        // ── D3D11 device + swap chain (same as NTSM) ─────────────────────
        let sd = DXGI_SWAP_CHAIN_DESC {
            BufferCount: 2,
            BufferDesc: DXGI_MODE_DESC {
                Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                RefreshRate: DXGI_RATIONAL { Numerator: 60, Denominator: 1 },
                ..Default::default()
            },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            OutputWindow: hwnd,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Windowed: BOOL(1),
            SwapEffect: DXGI_SWAP_EFFECT_DISCARD,
            Flags: DXGI_SWAP_CHAIN_FLAG_ALLOW_MODE_SWITCH.0 as u32,
        };

        let mut device: Option<ID3D11Device> = None;
        let mut context: Option<ID3D11DeviceContext> = None;
        let mut swap_chain: Option<IDXGISwapChain> = None;

        D3D11CreateDeviceAndSwapChain(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            None,
            D3D11_CREATE_DEVICE_FLAG(0),
            None,
            D3D11_SDK_VERSION,
            Some(&sd),
            Some(&mut swap_chain),
            Some(&mut device),
            None,
            Some(&mut context),
        )
        .expect("D3D11CreateDeviceAndSwapChain failed");

        G_DEVICE = device;
        G_CONTEXT = context;
        G_SWAPCHAIN = swap_chain;
        G_RTV = create_rtv();

        // ── ImGui context & backend init ─────────────────────────────────
        igCreateContext(std::ptr::null_mut());
        c_ImGui_DisableIni();   // ← prevents any .ini window from loading

        ig_set_config_flags(1);
        igStyleColorsDark(std::ptr::null_mut());

        c_ImGui_ImplWin32_Init(hwnd.0 as _);
        c_ImGui_ImplDX11_Init(
            G_DEVICE.as_ref().unwrap().as_raw() as *mut u8,
            G_CONTEXT.as_ref().unwrap().as_raw() as *mut u8,
        );

        APP_DATA = Some(Mutex::new(AppData {
            child: None,
            host: "127.0.0.1".to_string(),
            port: "5000".to_string(),
            status: "Stopped".to_string(),
        }));

        ShowWindow(hwnd, SW_SHOW);

        let mut msg = MSG::default();
        while G_RUNNING {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).into() {
                if msg.message == WM_QUIT { G_RUNNING = false; break; }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            if !G_RUNNING { break; }

            if let Some(data) = APP_DATA.as_ref() {
                let mut app = data.lock().unwrap();
                poll_child(&mut app);
            }

            c_ImGui_ImplDX11_NewFrame();
            c_ImGui_ImplWin32_NewFrame();
            igNewFrame();

            render_frame();

            igRender();
            let draw_data = igGetDrawData();
            if let Some(rtv) = G_RTV.as_ref() {
                G_CONTEXT.as_ref().unwrap().OMSetRenderTargets(Some(&[Some(rtv.clone())]), None);
                let clear = [
                    0x18 as f32 / 255.0,
                    0x18 as f32 / 255.0,
                    0x19 as f32 / 255.0,
                    1.0,
                ];
                G_CONTEXT.as_ref().unwrap().ClearRenderTargetView(rtv, &clear);
            }
            if !draw_data.is_null() {
                c_ImGui_ImplDX11_RenderDrawData(draw_data);
            }
            let _ = G_SWAPCHAIN.as_ref().unwrap().Present(1, DXGI_PRESENT(0));
        }

        // Clean up
        if let Some(data) = APP_DATA.take() {
            let mut app = data.lock().unwrap();
            stop_server_internal(&mut app);
        }
        c_ImGui_ImplDX11_Shutdown();
        c_ImGui_ImplWin32_Shutdown();
        igDestroyContext(std::ptr::null_mut());
    }
}