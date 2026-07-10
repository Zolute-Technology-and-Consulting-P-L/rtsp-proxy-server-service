// src/gui/imgui/imgui_capi.cpp
//
// Enterprise-grade DX11 + ImGui C-API bridge.
// Fixes:
//   1. SwapChain owned here — Present(1,0) for vsync, occlusion guard, device-lost recovery.
//   2. Frame-skip when occluded (window minimised / covered) → no GPU spin.
//   3. Clear colour applied every frame so backbuffer never goes stale → no white screen.
//   4. ImGui_Init() creates the DX11 device+swapchain; separate Init/Present/Shutdown exposed.

#include "imgui.h"
#include "imgui_impl_win32.h"
#include "imgui_impl_dx11.h"
#include <d3d11.h>
#include <dxgi.h>

#pragma comment(lib, "d3d11.lib")
#pragma comment(lib, "dxgi.lib")

extern IMGUI_IMPL_API LRESULT ImGui_ImplWin32_WndProcHandler(
    HWND hWnd, UINT msg, WPARAM wParam, LPARAM lParam);

// ── Internal DX11 state ────────────────────────────────────────────────────────
static ID3D11Device *g_pd3dDevice = nullptr;
static ID3D11DeviceContext *g_pd3dDeviceContext = nullptr;
static IDXGISwapChain *g_pSwapChain = nullptr;
static ID3D11RenderTargetView *g_mainRenderTargetView = nullptr;
static HWND g_hwnd = nullptr;
static bool g_occluded = false;

static bool CreateDeviceD3D(HWND hwnd);
static void CleanupDeviceD3D();
static void CreateRenderTarget();
static void CleanupRenderTarget();
static bool RecreateDevice();

// ── DX11 helpers ──────────────────────────────────────────────────────────────

static bool CreateDeviceD3D(HWND hwnd)
{
    DXGI_SWAP_CHAIN_DESC sd{};
    sd.BufferCount = 2;
    sd.BufferDesc.Width = 0;
    sd.BufferDesc.Height = 0;
    sd.BufferDesc.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
    sd.BufferDesc.RefreshRate.Numerator = 60;
    sd.BufferDesc.RefreshRate.Denominator = 1;
    sd.Flags = DXGI_SWAP_CHAIN_FLAG_ALLOW_MODE_SWITCH;
    sd.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
    sd.OutputWindow = hwnd;
    sd.SampleDesc.Count = 1;
    sd.SampleDesc.Quality = 0;
    sd.Windowed = TRUE;
    sd.SwapEffect = DXGI_SWAP_EFFECT_DISCARD;

    const D3D_FEATURE_LEVEL featureLevels[] = {
        D3D_FEATURE_LEVEL_11_0,
        D3D_FEATURE_LEVEL_10_0,
    };
    D3D_FEATURE_LEVEL featureLevel{};

    UINT createFlags = 0;
#ifdef _DEBUG
    createFlags |= D3D11_CREATE_DEVICE_DEBUG;
#endif

    HRESULT hr = D3D11CreateDeviceAndSwapChain(
        nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr,
        createFlags,
        featureLevels, (UINT)_countof(featureLevels),
        D3D11_SDK_VERSION,
        &sd, &g_pSwapChain,
        &g_pd3dDevice, &featureLevel,
        &g_pd3dDeviceContext);

    if (FAILED(hr))
        return false;

    CreateRenderTarget();
    return true;
}

static void CreateRenderTarget()
{
    ID3D11Texture2D *pBackBuffer = nullptr;
    g_pSwapChain->GetBuffer(0, IID_PPV_ARGS(&pBackBuffer));
    if (pBackBuffer)
    {
        g_pd3dDevice->CreateRenderTargetView(pBackBuffer, nullptr, &g_mainRenderTargetView);
        pBackBuffer->Release();
    }
}

static void CleanupRenderTarget()
{
    if (g_mainRenderTargetView)
    {
        g_mainRenderTargetView->Release();
        g_mainRenderTargetView = nullptr;
    }
}

static void CleanupDeviceD3D()
{
    CleanupRenderTarget();
    if (g_pSwapChain)
    {
        g_pSwapChain->Release();
        g_pSwapChain = nullptr;
    }
    if (g_pd3dDeviceContext)
    {
        g_pd3dDeviceContext->Release();
        g_pd3dDeviceContext = nullptr;
    }
    if (g_pd3dDevice)
    {
        g_pd3dDevice->Release();
        g_pd3dDevice = nullptr;
    }
}

// Full device-lost recovery — tears down ImGui backends, recreates DX11, reinits backends.
static bool RecreateDevice()
{
    ImGui_ImplDX11_Shutdown();
    CleanupDeviceD3D();

    if (!CreateDeviceD3D(g_hwnd))
        return false;

    ImGui_ImplDX11_Init(g_pd3dDevice, g_pd3dDeviceContext);
    return true;
}

// ── C-API ─────────────────────────────────────────────────────────────────────
extern "C"
{

    // ── Core ──────────────────────────────────────────────────────────────────────
    ImGuiContext *igCreateContext(ImFontAtlas *shared) { return ImGui::CreateContext(shared); }
    void igDestroyContext(ImGuiContext *ctx) { ImGui::DestroyContext(ctx); }
    void igNewFrame() { ImGui::NewFrame(); }
    void igRender() { ImGui::Render(); }
    ImDrawData *igGetDrawData() { return ImGui::GetDrawData(); }
    double igGetTime() { return ImGui::GetTime(); }

    // ── Window ────────────────────────────────────────────────────────────────────
    void igSetNextWindowPos(float x, float y, int cond, float px, float py)
    {
        ImGui::SetNextWindowPos(ImVec2(x, y), cond, ImVec2(px, py));
    }
    void igSetNextWindowSize(float w, float h, int cond)
    {
        ImGui::SetNextWindowSize(ImVec2(w, h), cond);
    }
    bool igBegin(const char *name, bool *p_open, ImGuiWindowFlags flags)
    {
        return ImGui::Begin(name, p_open, flags);
    }
    void igEnd() { ImGui::End(); }
    bool igBeginChild(const char *id, float w, float h, bool border, ImGuiWindowFlags flags)
    {
        return ImGui::BeginChild(id, ImVec2(w, h), border, flags);
    }
    void igEndChild() { ImGui::EndChild(); }
    void igGetWindowPos(float *x, float *y)
    {
        ImVec2 v = ImGui::GetWindowPos();
        *x = v.x;
        *y = v.y;
    }
    void igGetWindowSize(float *w, float *h)
    {
        ImVec2 v = ImGui::GetWindowSize();
        *w = v.x;
        *h = v.y;
    }

    // ── Text ──────────────────────────────────────────────────────────────────────
    void igText(const char *text) { ImGui::TextUnformatted(text); }
    void igTextColored(float r, float g, float b, float a, const char *text)
    {
        ImGui::TextColored(ImVec4(r, g, b, a), "%s", text);
    }
    void igTextWrapped(const char *text) { ImGui::TextWrapped("%s", text); }
    void igSetWindowFontScale(float scale) { ImGui::SetWindowFontScale(scale); }
    void igCalcTextSize(float *w, float *h, const char *text)
    {
        ImVec2 v = ImGui::CalcTextSize(text);
        *w = v.x;
        *h = v.y;
    }
    void igPushTextWrapPos(float x) { ImGui::PushTextWrapPos(x); }
    void igPopTextWrapPos() { ImGui::PopTextWrapPos(); }

    // ── Layout ────────────────────────────────────────────────────────────────────
    void igSeparator() { ImGui::Separator(); }
    void igSpacing() { ImGui::Spacing(); }
    void igDummy(float w, float h) { ImGui::Dummy(ImVec2(w, h)); }
    void igSameLine(float offset, float sp) { ImGui::SameLine(offset, sp); }
    void igSetNextItemWidth(float w) { ImGui::SetNextItemWidth(w); }
    float igGetCursorPosX() { return ImGui::GetCursorPosX(); }
    float igGetCursorPosY() { return ImGui::GetCursorPosY(); }
    void igSetCursorPosX(float x) { ImGui::SetCursorPosX(x); }
    void igSetCursorPosY(float y) { ImGui::SetCursorPosY(y); }
    void igSetCursorPos(float x, float y) { ImGui::SetCursorPos(ImVec2(x, y)); }
    void igGetContentRegionAvail(float *w, float *h)
    {
        ImVec2 v = ImGui::GetContentRegionAvail();
        *w = v.x;
        *h = v.y;
    }
    void igGetItemRectMin(float *x, float *y)
    {
        ImVec2 v = ImGui::GetItemRectMin();
        *x = v.x;
        *y = v.y;
    }
    void igGetItemRectMax(float *x, float *y)
    {
        ImVec2 v = ImGui::GetItemRectMax();
        *x = v.x;
        *y = v.y;
    }

    // ── Widgets ───────────────────────────────────────────────────────────────────
    bool igButton(const char *label, float w, float h)
    {
        return ImGui::Button(label, ImVec2(w, h));
    }
    bool igInvisibleButton(const char *id, float w, float h, int flags)
    {
        return ImGui::InvisibleButton(id, ImVec2(w, h), flags);
    }
    bool igInputText(const char *label, char *buf, size_t buf_size,
                     ImGuiInputTextFlags flags, ImGuiInputTextCallback cb, void *data)
    {
        return ImGui::InputText(label, buf, buf_size, flags, cb, data);
    }

    // ── Interaction ───────────────────────────────────────────────────────────────
    bool igIsItemHovered(int flags) { return ImGui::IsItemHovered(flags); }
    bool igIsItemActive() { return ImGui::IsItemActive(); }
    bool igIsItemClicked(int mb) { return ImGui::IsItemClicked(mb); }
    void igBeginDisabled(bool d) { ImGui::BeginDisabled(d); }
    void igEndDisabled() { ImGui::EndDisabled(); }
    void igPushID_Str(const char *id) { ImGui::PushID(id); }
    void igPopID() { ImGui::PopID(); }

    // ── Style ─────────────────────────────────────────────────────────────────────
    ImGuiStyle *igGetStyle() { return &ImGui::GetStyle(); }
    void igStyleColorsDark(ImGuiStyle *d) { ImGui::StyleColorsDark(d); }
    void igPushStyleColor_Vec4(int idx, float r, float g, float b, float a)
    {
        ImGui::PushStyleColor(idx, ImVec4(r, g, b, a));
    }
    void igPushStyleColor_U32(int idx, ImU32 col)
    {
        ImGui::PushStyleColor(idx, col);
    }
    void igPopStyleColor(int count) { ImGui::PopStyleColor(count); }
    void igPushStyleVar_Float(int idx, float val) { ImGui::PushStyleVar(idx, val); }
    void igPushStyleVar_Vec2(int idx, float x, float y) { ImGui::PushStyleVar(idx, ImVec2(x, y)); }
    void igPopStyleVar(int count) { ImGui::PopStyleVar(count); }
    ImU32 igColorConvertFloat4ToU32(float r, float g, float b, float a)
    {
        return ImGui::ColorConvertFloat4ToU32(ImVec4(r, g, b, a));
    }

    // ── DrawList ──────────────────────────────────────────────────────────────────
    ImDrawList *igGetWindowDrawList() { return ImGui::GetWindowDrawList(); }
    ImDrawList *igGetForegroundDrawList() { return ImGui::GetForegroundDrawList(); }

    void ImDrawList_AddLine(ImDrawList *dl, float p1x, float p1y, float p2x, float p2y,
                            ImU32 col, float thick)
    {
        dl->AddLine(ImVec2(p1x, p1y), ImVec2(p2x, p2y), col, thick);
    }

    void ImDrawList_AddRect(ImDrawList *dl, float x1, float y1, float x2, float y2,
                            ImU32 col, float rounding, int flags, float thick)
    {
        dl->AddRect(ImVec2(x1, y1), ImVec2(x2, y2), col, rounding, flags, thick);
    }

    void ImDrawList_AddRectFilled(ImDrawList *dl, float x1, float y1, float x2, float y2,
                                  ImU32 col, float rounding, int flags)
    {
        dl->AddRectFilled(ImVec2(x1, y1), ImVec2(x2, y2), col, rounding, flags);
    }

    void ImDrawList_AddRectFilledMultiColor(ImDrawList *dl,
                                            float x1, float y1, float x2, float y2,
                                            ImU32 col_tl, ImU32 col_tr, ImU32 col_br, ImU32 col_bl)
    {
        dl->AddRectFilledMultiColor(ImVec2(x1, y1), ImVec2(x2, y2), col_tl, col_tr, col_br, col_bl);
    }

    void ImDrawList_AddCircle(ImDrawList *dl, float cx, float cy, float r,
                              ImU32 col, int segs, float thick)
    {
        dl->AddCircle(ImVec2(cx, cy), r, col, segs, thick);
    }

    void ImDrawList_AddCircleFilled(ImDrawList *dl, float cx, float cy, float r, ImU32 col, int segs)
    {
        dl->AddCircleFilled(ImVec2(cx, cy), r, col, segs);
    }

    void ImDrawList_PushClipRect(ImDrawList *dl, float min_x, float min_y,
                                 float max_x, float max_y, bool intersect)
    {
        dl->PushClipRect(ImVec2(min_x, min_y), ImVec2(max_x, max_y), intersect);
    }

    void ImDrawList_PopClipRect(ImDrawList *dl) { dl->PopClipRect(); }

    void ImDrawList_AddText(ImDrawList *dl, float x, float y, ImU32 col, const char *text)
    {
        dl->AddText(ImVec2(x, y), col, text);
    }

    void ImDrawList_AddText_Vec2(ImDrawList *dl, float x, float y, ImU32 col, const char *text)
    {
        dl->AddText(ImVec2(x, y), col, text);
    }

    void igGetCursorScreenPos(float *x, float *y)
    {
        ImVec2 v = ImGui::GetCursorScreenPos();
        *x = v.x;
        *y = v.y;
    }

    // ── Backend: Win32 & DX11 initialisation (SwapChain owned here) ──────────────

    // Call once at startup. Creates the DX11 device + swap chain and inits backends.
    bool ImGui_Init(HWND hwnd)
    {
        g_hwnd = hwnd;
        if (!CreateDeviceD3D(hwnd))
            return false;

        IMGUI_CHECKVERSION();
        ImGui::CreateContext();
        ImGui::StyleColorsDark();

        if (!ImGui_ImplWin32_Init(hwnd))
            return false;
        if (!ImGui_ImplDX11_Init(g_pd3dDevice, g_pd3dDeviceContext))
            return false;

        return true;
    }

    // Resize the swapchain buffers — call from WM_SIZE.
    void ImGui_ResizeBuffers(UINT width, UINT height)
    {
        if (!g_pSwapChain)
            return;
        CleanupRenderTarget();
        g_pSwapChain->ResizeBuffers(0, width, height, DXGI_FORMAT_UNKNOWN, 0);
        CreateRenderTarget();
    }

    // Begin-frame: returns false when the window is occluded → caller should skip rendering.
    bool ImGui_BeginFrame()
    {
        // Test for occlusion without blocking.
        if (g_occluded)
        {
            HRESULT hr = g_pSwapChain->Present(0, DXGI_PRESENT_TEST);
            if (hr == S_OK)
                g_occluded = false;
            else
                return false; // still occluded — skip this frame
        }

        ImGui_ImplDX11_NewFrame();
        ImGui_ImplWin32_NewFrame();
        ImGui::NewFrame();
        return true;
    }

    // End-frame: renders ImGui and presents to screen.
    // Returns false on device-lost so the Rust side can call ImGui_Init again.
    bool ImGui_EndFrame()
    {
        ImGui::Render();

        // Clear backbuffer every frame → prevents stale/white image when minimised then restored.
        const float clear_color[4] = {0.05f, 0.05f, 0.05f, 1.00f};
        g_pd3dDeviceContext->OMSetRenderTargets(1, &g_mainRenderTargetView, nullptr);
        g_pd3dDeviceContext->ClearRenderTargetView(g_mainRenderTargetView, clear_color);

        ImGui_ImplDX11_RenderDrawData(ImGui::GetDrawData());

        // Present with vsync interval 1 → GPU governs frame pacing, CPU never spins.
        HRESULT hr = g_pSwapChain->Present(1, 0);

        if (hr == DXGI_STATUS_OCCLUDED)
        {
            g_occluded = true;
            return true; // not an error — just invisible
        }

        if (hr == DXGI_ERROR_DEVICE_REMOVED || hr == DXGI_ERROR_DEVICE_RESET)
        {
            // Attempt automatic recovery once.
            if (!RecreateDevice())
                return false; // irrecoverable — Rust side should exit
        }

        return true;
    }

    void ImGui_Shutdown()
    {
        ImGui_ImplDX11_Shutdown();
        ImGui_ImplWin32_Shutdown();
        ImGui::DestroyContext();
        CleanupDeviceD3D();
    }

    // ── Thin wrappers still needed by imgui_ffi.rs ────────────────────────────────
    void ig_set_config_flags(int flags) { ImGui::GetIO().ConfigFlags |= flags; }
    float ig_get_display_w() { return ImGui::GetIO().DisplaySize.x; }
    float ig_get_display_h() { return ImGui::GetIO().DisplaySize.y; }

    bool c_ImGui_ImplWin32_Init(void *hwnd) { return ImGui_ImplWin32_Init(hwnd); }
    void c_ImGui_ImplWin32_Shutdown() { ImGui_ImplWin32_Shutdown(); }
    void c_ImGui_ImplWin32_NewFrame() { ImGui_ImplWin32_NewFrame(); }

    bool c_ImGui_ImplDX11_Init(void *device, void *ctx)
    {
        return ImGui_ImplDX11_Init(
            static_cast<ID3D11Device *>(device),
            static_cast<ID3D11DeviceContext *>(ctx));
    }
    void c_ImGui_ImplDX11_Shutdown() { ImGui_ImplDX11_Shutdown(); }
    void c_ImGui_ImplDX11_NewFrame() { ImGui_ImplDX11_NewFrame(); }
    void c_ImGui_ImplDX11_RenderDrawData(ImDrawData *d) { ImGui_ImplDX11_RenderDrawData(d); }

    // ── IO & Fonts ────────────────────────────────────────────────────────────────
    void *c_igGetIO() { return &ImGui::GetIO(); }
    void *c_igGetFonts(void *io) { return ((ImGuiIO *)io)->Fonts; }

    void c_ImFontAtlas_AddEmbeddedFont(void *atlas, const void *font_data,
                                       int font_size, float size_pixels)
    {
        ImFontConfig cfg;
        cfg.FontDataOwnedByAtlas = false;
        ((ImFontAtlas *)atlas)->AddFontFromMemoryTTF((void *)font_data, font_size, size_pixels, &cfg, nullptr);
    }

    // Load a font from a file (system path) with custom glyph ranges.
    // Returns the ImFont pointer (Rust can ignore it).
    void *c_ImFontAtlas_AddFontFromFileTTF(
        void *atlas, const char *filename,
        float size_pixels,
        const ImFontConfig *font_cfg,
        const unsigned short *glyph_ranges)
    {
        ImFontAtlas *a = (ImFontAtlas *)atlas;
        return a->AddFontFromFileTTF(filename, size_pixels,
                                     font_cfg ? font_cfg : nullptr,
                                     glyph_ranges);
    }

    // Disable the default imgui.ini file
    void c_ImGui_DisableIni()
    {
        ImGui::GetIO().IniFilename = nullptr;
    }

} // extern "C"

extern "C" LRESULT c_ImGui_ImplWin32_WndProcHandler(
    HWND hwnd, UINT msg, WPARAM wp, LPARAM lp)
{
    return ImGui_ImplWin32_WndProcHandler(hwnd, msg, wp, lp);
}