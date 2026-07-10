// src/gui/imgui/imgui_ffi.rs
//
// Zero-cost C-ABI bridge to imgui_capi.cpp.
// New high-level entry points: ImGui_Init, ImGui_BeginFrame, ImGui_EndFrame,
// ImGui_ResizeBuffers, ImGui_Shutdown — all DX11/SwapChain management is in C++.

use std::os::raw::{c_char, c_void};

extern "C" {
    // ── Lifecycle (new — replaces manual DX11 init) ────────────────────────────
    /// Creates the DX11 device, swap chain, and inits Win32+DX11 backends.
    /// Returns false on failure.
    pub fn ImGui_Init(hwnd: isize) -> bool;

    /// Must be called on WM_SIZE to resize swap chain buffers.
    pub fn ImGui_ResizeBuffers(width: u32, height: u32);

    /// Begins a new frame. Returns false when the window is occluded;
    /// caller must skip the render and try again next tick.
    pub fn ImGui_BeginFrame() -> bool;

    /// Renders ImGui draw data, clears the backbuffer, and calls Present(1,0).
    /// Returns false on unrecoverable device-lost.
    pub fn ImGui_EndFrame() -> bool;

    /// Tears down ImGui and releases all DX11 resources.
    pub fn ImGui_Shutdown();

    // ── Core ──────────────────────────────────────────────────────────────────
    pub fn igCreateContext(shared_font_atlas: *mut u8) -> *mut u8;
    pub fn igDestroyContext(ctx: *mut u8);
    pub fn igNewFrame();
    pub fn igRender();
    pub fn igGetDrawData() -> *mut u8;
    pub fn igGetTime() -> f64;

    // ── Window ────────────────────────────────────────────────────────────────
    pub fn igSetNextWindowPos(x: f32, y: f32, cond: i32, px: f32, py: f32);
    pub fn igSetNextWindowSize(w: f32, h: f32, cond: i32);
    pub fn igBegin(name: *const c_char, p_open: *mut bool, flags: i32) -> bool;
    pub fn igEnd();
    pub fn igBeginChild(str_id: *const c_char, w: f32, h: f32, border: bool, flags: i32) -> bool;
    pub fn igEndChild();
    pub fn igGetWindowPos(x: *mut f32, y: *mut f32);
    pub fn igGetWindowSize(w: *mut f32, h: *mut f32);

    // ── Text ──────────────────────────────────────────────────────────────────
    pub fn igText(fmt: *const c_char);
    pub fn igTextColored(r: f32, g: f32, b: f32, a: f32, fmt: *const c_char);
    pub fn igTextWrapped(fmt: *const c_char);
    pub fn igSetWindowFontScale(scale: f32);
    pub fn igCalcTextSize(w: *mut f32, h: *mut f32, text: *const c_char);
    pub fn igPushTextWrapPos(wrap_local_pos_x: f32);
    pub fn igPopTextWrapPos();

    // ── Layout ────────────────────────────────────────────────────────────────
    pub fn igSeparator();
    pub fn igSpacing();
    pub fn igDummy(w: f32, h: f32);
    pub fn igSameLine(offset_from_start_x: f32, spacing: f32);
    pub fn igSetNextItemWidth(item_width: f32);
    pub fn igGetCursorPosX() -> f32;
    pub fn igGetCursorPosY() -> f32;
    pub fn igSetCursorPosX(x: f32);
    pub fn igSetCursorPosY(y: f32);
    pub fn igSetCursorPos(x: f32, y: f32);
    pub fn igGetContentRegionAvail(w: *mut f32, h: *mut f32);
    pub fn igGetItemRectMin(x: *mut f32, y: *mut f32);
    pub fn igGetItemRectMax(x: *mut f32, y: *mut f32);

    // ── Widgets ───────────────────────────────────────────────────────────────
    pub fn igButton(label: *const c_char, w: f32, h: f32) -> bool;
    pub fn igInvisibleButton(str_id: *const c_char, w: f32, h: f32, flags: i32) -> bool;
    pub fn igInputText(
        label: *const c_char,
        buf: *mut c_char,
        buf_size: usize,
        flags: i32,
        cb: Option<unsafe extern "C" fn(*mut u8) -> i32>,
        data: *mut c_void,
    ) -> bool;

    // ── Interaction ───────────────────────────────────────────────────────────
    pub fn igIsItemHovered(flags: i32) -> bool;
    pub fn igIsItemActive() -> bool;
    pub fn igIsItemClicked(mouse_button: i32) -> bool;
    pub fn igBeginDisabled(disabled: bool);
    pub fn igEndDisabled();
    pub fn igPushID_Str(str_id: *const c_char);
    pub fn igPopID();

    // ── Style ─────────────────────────────────────────────────────────────────
    pub fn igGetStyle() -> *mut c_void;
    pub fn igStyleColorsDark(dst: *mut c_void);
    pub fn igPushStyleColor_Vec4(idx: i32, r: f32, g: f32, b: f32, a: f32);
    pub fn igPushStyleColor_U32(idx: i32, col: u32);
    pub fn igPopStyleColor(count: i32);
    pub fn igPushStyleVar_Float(idx: i32, val: f32);
    pub fn igPushStyleVar_Vec2(idx: i32, x: f32, y: f32);
    pub fn igPopStyleVar(count: i32);
    pub fn igColorConvertFloat4ToU32(r: f32, g: f32, b: f32, a: f32) -> u32;

    // ── DrawList ──────────────────────────────────────────────────────────────
    pub fn igGetWindowDrawList() -> *mut c_void;
    pub fn igGetForegroundDrawList() -> *mut c_void;
    pub fn ImDrawList_AddLine(
        list: *mut c_void,
        p1x: f32,
        p1y: f32,
        p2x: f32,
        p2y: f32,
        col: u32,
        thickness: f32,
    );
    pub fn ImDrawList_AddRect(
        list: *mut c_void,
        minx: f32,
        miny: f32,
        maxx: f32,
        maxy: f32,
        col: u32,
        rounding: f32,
        flags: i32,
        thickness: f32,
    );
    pub fn ImDrawList_AddRectFilled(
        list: *mut c_void,
        minx: f32,
        miny: f32,
        maxx: f32,
        maxy: f32,
        col: u32,
        rounding: f32,
        flags: i32,
    );
    pub fn ImDrawList_AddRectFilledMultiColor(
        list: *mut c_void,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        col_tl: u32,
        col_tr: u32,
        col_br: u32,
        col_bl: u32,
    );
    pub fn ImDrawList_AddCircle(
        list: *mut c_void,
        cx: f32,
        cy: f32,
        radius: f32,
        col: u32,
        num_segments: i32,
        thickness: f32,
    );
    pub fn ImDrawList_AddCircleFilled(
        list: *mut c_void,
        cx: f32,
        cy: f32,
        radius: f32,
        col: u32,
        num_segments: i32,
    );
    pub fn ImDrawList_AddText(list: *mut c_void, x: f32, y: f32, col: u32, text: *const c_char);
    pub fn ImDrawList_AddText_Vec2(
        draw_list: *mut c_void,
        pos_x: f32,
        pos_y: f32,
        col: u32,
        text: *const c_char,
    );
    pub fn ImDrawList_PushClipRect(
        list: *mut c_void,
        min_x: f32,
        min_y: f32,
        max_x: f32,
        max_y: f32,
        intersect_with_current: bool,
    );
    pub fn ImDrawList_PopClipRect(list: *mut c_void);
    pub fn igGetCursorScreenPos(x: *mut f32, y: *mut f32);

    // ── Backend (legacy thin wrappers kept for compatibility) ─────────────────
    pub fn ig_set_config_flags(flags: i32);
    pub fn ig_get_display_w() -> f32;
    pub fn ig_get_display_h() -> f32;
    pub fn c_ImGui_ImplWin32_Init(hwnd: *mut u8) -> bool;
    pub fn c_ImGui_ImplWin32_Shutdown();
    pub fn c_ImGui_ImplWin32_NewFrame();
    pub fn c_ImGui_ImplWin32_WndProcHandler(hwnd: isize, msg: u32, wp: usize, lp: isize) -> isize;
    pub fn c_ImGui_ImplDX11_Init(device: *mut u8, ctx: *mut u8) -> bool;
    pub fn c_ImGui_ImplDX11_Shutdown();
    pub fn c_ImGui_ImplDX11_NewFrame();
    pub fn c_ImGui_ImplDX11_RenderDrawData(draw_data: *mut u8);

    // ── IO & Fonts ────────────────────────────────────────────────────────────
    pub fn c_igGetIO() -> *mut c_void;
    pub fn c_igGetFonts(io: *mut c_void) -> *mut c_void;
    pub fn c_ImFontAtlas_AddEmbeddedFont(
        atlas: *mut c_void,
        font_data: *const c_void,
        font_size: i32,
        size_pixels: f32,
    );

    pub fn c_ImFontAtlas_AddFontFromFileTTF(
        atlas: *mut c_void,
        filename: *const c_char,
        size_pixels: f32,
        font_cfg: *const c_void,
        glyph_ranges: *const u16,
    ) -> *mut c_void;

    pub fn c_ImGui_DisableIni();

}
