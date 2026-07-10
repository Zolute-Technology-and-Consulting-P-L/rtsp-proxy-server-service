//src/gui/components/text_field.rs

use crate::gui::imgui::imgui_ffi::*;
use std::ffi::CString;

pub const SV_FRAME_PADDING: i32 = 11; 

/// Renders a modern text field. Assumes the label is drawn externally in the left column.
pub unsafe fn render_text_field(
    id:    &str,
    ptr:   *mut std::os::raw::c_char,
    size:  usize,
    width: f32,
) -> bool {
    let full_id = CString::new(format!("##{}", id)).unwrap();

    igSetNextItemWidth(width);
    
    // Increased Y from 8.0 to 14.0 to make the fields chunkier and more modern
    igPushStyleVar_Vec2(SV_FRAME_PADDING, 12.0, 14.0);
    
    let changed = igInputText(
        full_id.as_ptr(), ptr, size, 0, None, std::ptr::null_mut(),
    );
    
    igPopStyleVar(1);
    
    changed
}