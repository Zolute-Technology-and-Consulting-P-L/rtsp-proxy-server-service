//src/gui/components/gradient_button.rs

use crate::gui::imgui::imgui_ffi::*;
use super::constants::*;
use std::ffi::CString;

/// Draws a solid rounded rect (previously gradient, simplified to fix clipping).
pub unsafe fn draw_gradient_rect(
    dl:      *mut std::ffi::c_void,
    x1: f32, y1: f32,
    x2: f32, y2: f32,
    hovered: bool,
    active:  bool,
) {
    let top = if active {
        GRAD_TOP_ACT
    } else if hovered {
        GRAD_TOP_HOV
    } else {
        GRAD_TOP
    };

    let rounding = 8.0f32;

    // Rounded mask rect in the top colour — establishes the rounded shape
    // Removed the MultiColor overdraw because ImGui doesn't support rounded multi-color rects natively.
    ImDrawList_AddRectFilled(dl, x1, y1, x2, y2, top, rounding, ROUND_ALL);
}

/// Renders a gradient-filled button using InvisibleButton for hit-testing
/// and manual draw-list calls for visuals, so text always renders on top.
pub unsafe fn gradient_button(
    dl:    *mut std::ffi::c_void,
    label: &str,
    w:     f32,
    h:     f32,
) -> bool {
    // Capture screen pos before InvisibleButton advances the cursor
    let mut sx = 0.0f32;
    let mut sy = 0.0f32;
    igGetCursorScreenPos(&mut sx, &mut sy);

    // Hit-test region — InvisibleButton submits no visuals, just the AABB
    let lbl_id = cs(&format!("##{}_hit", label));
    igPushStyleVar_Float(SV_FRAME_ROUNDING, 8.0);
    let hit = igInvisibleButton(lbl_id.as_ptr(), w, h, 0);
    let hov = igIsItemHovered(0);
    let act = igIsItemActive();
    igPopStyleVar(1);

    // Background shape (drawn after InvisibleButton so hover/active state is known)
    draw_gradient_rect(dl, sx, sy, sx + w, sy + h, hov, act);

    // Bold label: draw text twice — offset by 1px horizontally for synthetic bold
    let lbl     = cs(label);
    let mut tw  = 0.0f32;
    let mut th  = 0.0f32;
    igCalcTextSize(&mut tw, &mut th, lbl.as_ptr());
    let tx = sx + (w - tw) * 0.5;
    let ty = sy + (h - th) * 0.5;
    let col = im_col32(245, 245, 245, 255);
    ImDrawList_AddText(dl, tx,        ty, col, lbl.as_ptr()); // base pass
    ImDrawList_AddText(dl, tx + 1.0,  ty, col, lbl.as_ptr()); // +1px bold pass

    hit
}

/// Renders a dark solid button (secondary/destructive actions).
pub unsafe fn dark_button(
    label:      &str,
    w:          f32,
    h:          f32,
    text_color: (f32, f32, f32, f32),
) -> bool {
    igPushStyleColor_Vec4(IM_COL_BUTTON,         0.13, 0.13, 0.15, 1.0);
    igPushStyleColor_Vec4(IM_COL_BUTTON_HOVERED, 0.20, 0.20, 0.24, 1.0);
    igPushStyleColor_Vec4(IM_COL_BUTTON_ACTIVE,  0.10, 0.10, 0.12, 1.0);
    igPushStyleColor_Vec4(IM_COL_TEXT,
        text_color.0, text_color.1, text_color.2, text_color.3);
    igPushStyleVar_Float(SV_FRAME_ROUNDING, 8.0);

    let clicked = igButton(cs(label).as_ptr(), w, h);

    igPopStyleVar(1);
    igPopStyleColor(4);

    clicked
}

pub fn cs(s: &str) -> CString {
    CString::new(s).unwrap_or_else(|_| CString::new("<?>").unwrap())
}