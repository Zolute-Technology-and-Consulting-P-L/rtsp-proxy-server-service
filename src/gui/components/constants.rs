// ── ImGuiCol indices ──────────────────────────────────────────────────────────
pub const IM_COL_WINDOW_BG:      i32 = 2;
pub const IM_COL_BORDER:         i32 = 5;
pub const IM_COL_BUTTON:         i32 = 21;
pub const IM_COL_BUTTON_HOVERED: i32 = 22;
pub const IM_COL_BUTTON_ACTIVE:  i32 = 23;
pub const IM_COL_TEXT:           i32 = 0;

// ── ImGuiStyleVar indices ─────────────────────────────────────────────────────
pub const SV_WINDOW_PADDING:    i32 = 2;
pub const SV_WINDOW_ROUNDING:   i32 = 3;
pub const SV_WINDOW_BORDERSIZE: i32 = 4;
pub const SV_FRAME_ROUNDING:    i32 = 12;

// ── Window flags ──────────────────────────────────────────────────────────────
pub const WIN_FLAGS: i32 = (1<<0)|(1<<1)|(1<<2)|(1<<3)|(1<<4)|(1<<5)|(1<<7);

// ── Window size ───────────────────────────────────────────────────────────────
pub const WIN_W: f32 = 460.0;
pub const WIN_H: f32 = 320.0; // Reverted to 320 to stop squishing

// ── Layout — buttons moved WAY UP from the bottom ─────────────────────────────
pub const BTN_H:     f32 = 40.0;
// Pushed way up (subtracting 80px) so you can test the extreme visual margin
pub const BTN_ROW_Y: f32 = WIN_H - BTN_H - 50.0; 

pub const ICON_CY:   f32 = 72.0;
pub const TITLE_Y:   f32 = ICON_CY + 68.0;   // 120
pub const SUBTEXT_Y: f32 = TITLE_Y + 20.0;   // 140

pub const CONNECT_BTN_W: f32 = 236.0;
pub const GEAR_SIZE:     f32 = 40.0;
pub const ROW_W:         f32 = CONNECT_BTN_W + 8.0 + GEAR_SIZE;
pub const CONNECT_X:     f32 = (WIN_W - ROW_W) * 0.5;

pub const GRAPH_H:      f32 = 110.0;
pub const CONN_LABEL_Y: f32 = GRAPH_H + 4.0;
pub const CARD_TOP:     f32 = GRAPH_H + 22.0;
pub const CARD_H:       f32 = 58.0;
pub const ACTION_BTN_W: f32 = (WIN_W - 28.0 - 10.0 - 28.0) * 0.5;

// ── Colours (ABGR u32) ────────────────────────────────────────────────────────
pub const fn im_col32(r: u32, g: u32, b: u32, a: u32) -> u32 {
    (a << 24) | (b << 16) | (g << 8) | r
}

pub const DC_BG:     u32 = im_col32(0x18, 0x18, 0x19, 255); // #181819
pub const DC_CARD:   u32 = im_col32( 26,  26,  30, 255);
pub const DC_BORDER: u32 = im_col32( 46,  46,  52, 255);
pub const DC_MUTED:  u32 = im_col32(150, 150, 160, 255);

pub const GRAD_TOP:     u32 = im_col32( 93, 190, 255, 255);
pub const GRAD_BOT:     u32 = im_col32( 53, 160, 255, 255);
pub const GRAD_TOP_HOV: u32 = im_col32(130, 210, 255, 255);
pub const GRAD_BOT_HOV: u32 = im_col32( 80, 180, 255, 255);
pub const GRAD_TOP_ACT: u32 = im_col32( 60, 155, 230, 255);
pub const GRAD_BOT_ACT: u32 = im_col32( 30, 120, 210, 255);

pub const PRIMARY: (f32,f32,f32,f32) = (245./255., 245./255., 245./255., 1.0);
pub const MUTED:   (f32,f32,f32,f32) = (150./255., 150./255., 160./255., 1.0);
pub const SUCCESS: (f32,f32,f32,f32) = (0.30, 0.69, 0.31, 1.0);
pub const ERROR:   (f32,f32,f32,f32) = (1.0,  0.4,  0.4,  1.0);

pub const ROUND_ALL: i32 = 0xF0;

pub const fn im_col32_alpha(r: u8, g: u8, b: u8, a: u8) -> u32 {
    (a as u32) << 24 | (b as u32) << 16 | (g as u32) << 8 | (r as u32)
}

pub const SIDE_MARGIN: f32 = 20.0;       // uniform outer padding
pub const MAX_CARD_WIDTH: f32 = 520.0;


// ── Text‑field positioning ─────────────────────────────────────────────────
pub const LABEL_X: f32 = 130.0;          // x‑position of input fields after the label

// ── Additional ImGuiCol indices ─────────────────────────────────────────────
pub const IM_COL_FRAME_BG:      i32 = 7;
pub const IM_COL_FRAME_BG_HOV:  i32 = 8;
pub const IM_COL_FRAME_BG_ACT:  i32 = 9;