use ratatui::style::Color;

// Base surfaces — Catppuccin Mocha inspired: dark blue-slate, never pure black.
pub const BG: Color = Color::Rgb(0x1e, 0x1e, 0x2e);
pub const PANEL: Color = Color::Rgb(0x2a, 0x2b, 0x3d);
pub const SEL_BG: Color = Color::Rgb(0x35, 0x37, 0x4a);
pub const BORDER: Color = Color::Rgb(0x45, 0x47, 0x5a);

// Text
pub const FG: Color = Color::Rgb(0xcd, 0xd6, 0xf4);
pub const FG_BRIGHT: Color = Color::Rgb(0xff, 0xff, 0xff);
pub const GRAY: Color = Color::Rgb(0xa6, 0xad, 0xc8);
pub const DIM: Color = Color::Rgb(0x6c, 0x70, 0x86);

// Accents
pub const ACCENT: Color = Color::Rgb(0xfa, 0xb3, 0x87); // peach
pub const YELLOW: Color = Color::Rgb(0xf9, 0xe2, 0xaf);
pub const RED: Color = Color::Rgb(0xf3, 0x8b, 0xa8);
pub const MAUVE: Color = Color::Rgb(0xcb, 0xa6, 0xf7);

const SOURCE_PALETTE: [Color; 8] = [
    Color::Rgb(0xa6, 0xe3, 0xa1), // green
    Color::Rgb(0x94, 0xe2, 0xd5), // teal
    Color::Rgb(0x89, 0xdc, 0xeb), // sky
    Color::Rgb(0x89, 0xb4, 0xfa), // blue
    Color::Rgb(0xb4, 0xbe, 0xfe), // lavender
    Color::Rgb(0xf5, 0xc2, 0xe7), // pink
    Color::Rgb(0xf2, 0xcd, 0xcd), // flamingo
    Color::Rgb(0xf5, 0xe0, 0xdc), // rosewater
];

/// Deterministic, stable-across-runs color for a source domain (FNV-1a hash,
/// since std's HashMap hasher is randomly seeded per-process). Used to tint
/// the domain name in the article meta line.
pub fn source_color(domain: &str) -> Color {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in domain.bytes() {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    SOURCE_PALETTE[(hash as usize) % SOURCE_PALETTE.len()]
}
