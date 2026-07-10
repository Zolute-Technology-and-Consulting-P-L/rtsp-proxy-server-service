use std::path::PathBuf;

fn main() {
    // Only run on Windows
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap() != "windows" {
        return;
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let third_party = manifest_dir.join("third_party").join("imgui");
    let imgui_capi = manifest_dir
        .join("src")
        .join("gui")
        .join("imgui")
        .join("imgui_capi.cpp");

    // Rebuild if any source changes
    println!("cargo:rerun-if-changed={}", third_party.join("imgui.cpp").display());
    println!("cargo:rerun-if-changed={}", third_party.join("imgui_draw.cpp").display());
    println!("cargo:rerun-if-changed={}", third_party.join("imgui_tables.cpp").display());
    println!("cargo:rerun-if-changed={}", third_party.join("imgui_widgets.cpp").display());
    println!("cargo:rerun-if-changed={}", third_party.join("imgui_impl_win32.cpp").display());
    println!("cargo:rerun-if-changed={}", third_party.join("imgui_impl_dx11.cpp").display());
    println!("cargo:rerun-if-changed={}", imgui_capi.display());

    // Compile ImGui C++ files
    cc::Build::new()
        .cpp(true)
        .flag("/EHsc")
        .include(&third_party)
        .file(third_party.join("imgui.cpp"))
        .file(third_party.join("imgui_draw.cpp"))
        .file(third_party.join("imgui_tables.cpp"))
        .file(third_party.join("imgui_widgets.cpp"))
        .file(third_party.join("imgui_impl_win32.cpp"))
        .file(third_party.join("imgui_impl_dx11.cpp"))
        .file(&imgui_capi)
        .compile("imgui");

    // Link DirectX and DWM
    println!("cargo:rustc-link-lib=d3d11");
    println!("cargo:rustc-link-lib=dxgi");
    println!("cargo:rustc-link-lib=dwmapi");

    // Embed Windows resources (icon + metadata)
    let icon_path = manifest_dir.join("assets").join("logo.ico");
    if let Err(e) = winres::WindowsResource::new()
        .set_icon(icon_path.to_str().unwrap())
        .set("FileDescription", "RTSP Proxy Server")
        .set("ProductName", "RTSP Proxy")
        .set("OriginalFilename", "rtsp-proxy.exe")
        .set("CompanyName", "Vibgyor")
        .set("LegalCopyright", "Copyright (c) Vibgyor")
        .compile()
    {
        println!("cargo:warning=Failed to embed Windows resources: {}", e);
    }
}