//! The window/taskbar icon, drawn procedurally: five colour pips arranged in
//! a ring on a dark rounded tile. Deliberately a generic five-colour mark
//! rather than any trademarked Magic artwork.

const SIZE: u32 = 64;

/// WUBRG, in the usual clockwise order starting at the top.
const PIPS: [(u8, u8, u8); 5] = [
    (0xF6, 0xF1, 0xDF), // white
    (0x5B, 0x9B, 0xD5), // blue
    (0x2A, 0x2A, 0x33), // black
    (0xC6, 0x4B, 0x3F), // red
    (0x5F, 0xA8, 0x6B), // green
];

pub fn window_icon() -> Option<iced::window::Icon> {
    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];
    let center = SIZE as f32 / 2.0;
    let ring_radius = SIZE as f32 * 0.29;
    let pip_radius = SIZE as f32 * 0.13;
    let corner = SIZE as f32 * 0.22;

    for y in 0..SIZE {
        for x in 0..SIZE {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let idx = ((y * SIZE + x) * 4) as usize;

            if !inside_rounded_square(fx, fy, SIZE as f32, corner) {
                continue;
            }

            // Base tile, with the gold ring drawn onto it.
            let mut pixel = (0x14u8, 0x13u8, 0x1Cu8);
            let dist_from_center = ((fx - center).powi(2) + (fy - center).powi(2)).sqrt();
            if (dist_from_center - ring_radius).abs() < 1.6 {
                pixel = (0xC8, 0xA2, 0x5C);
            }

            for (i, colour) in PIPS.iter().enumerate() {
                let angle =
                    -std::f32::consts::FRAC_PI_2 + (i as f32) * std::f32::consts::TAU / 5.0;
                let px = center + ring_radius * angle.cos();
                let py = center + ring_radius * angle.sin();
                let d = ((fx - px).powi(2) + (fy - py).powi(2)).sqrt();
                if d < pip_radius {
                    pixel = *colour;
                } else if d < pip_radius + 1.4 {
                    pixel = (0xC8, 0xA2, 0x5C);
                }
            }

            rgba[idx] = pixel.0;
            rgba[idx + 1] = pixel.1;
            rgba[idx + 2] = pixel.2;
            rgba[idx + 3] = 0xFF;
        }
    }

    iced::window::icon::from_rgba(rgba, SIZE, SIZE).ok()
}

fn inside_rounded_square(x: f32, y: f32, size: f32, corner: f32) -> bool {
    let cx = x.min(size - x);
    let cy = y.min(size - y);
    if cx >= corner || cy >= corner {
        return true;
    }
    let dx = corner - cx;
    let dy = corner - cy;
    (dx * dx + dy * dy).sqrt() <= corner
}
