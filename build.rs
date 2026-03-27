use std::path::Path;

fn main() {
    // Generate icons BEFORE compiling Slint (Slint references ui/icon.png)
    generate_tray_icon();
    generate_app_icon();

    slint_build::compile("ui/main.slint").unwrap();
    println!("cargo::rerun-if-changed=build.rs");
}

/// Distance from point (px, py) to line segment (ax,ay)-(bx,by)
fn dist_to_segment(px: f64, py: f64, ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    let dx = bx - ax;
    let dy = by - ay;
    let len_sq = dx * dx + dy * dy;
    if len_sq == 0.0 {
        return ((px - ax).powi(2) + (py - ay).powi(2)).sqrt();
    }
    let t = ((px - ax) * dx + (py - ay) * dy) / len_sq;
    let t = t.clamp(0.0, 1.0);
    let proj_x = ax + t * dx;
    let proj_y = ay + t * dy;
    ((px - proj_x).powi(2) + (py - proj_y).powi(2)).sqrt()
}

/// Draw a clean arrow using distance fields for anti-aliased edges.
/// Arrow shape: diagonal shaft from bottom-left to top-right,
/// with an arrowhead (horizontal + vertical bar forming an L at the tip).
fn draw_arrow_sdf(
    img: &mut image::RgbaImage,
    size: u32,
    padding: f64,
    thickness: f64,
    color: [u8; 3],
) {
    let s = size as f64;
    let inner = s - 2.0 * padding;

    // Arrow coordinates in normalized space, scaled to inner area
    // Shaft: bottom-left to top-right
    let shaft_x0 = padding + inner * 0.12;
    let shaft_y0 = padding + inner * 0.88;
    let shaft_x1 = padding + inner * 0.82;
    let shaft_y1 = padding + inner * 0.18;

    // Arrowhead horizontal bar: from tip going left
    let head_h_x0 = padding + inner * 0.45;
    let head_h_y0 = padding + inner * 0.18;
    let head_h_x1 = padding + inner * 0.82;
    let head_h_y1 = padding + inner * 0.18;

    // Arrowhead vertical bar: from tip going down
    let head_v_x0 = padding + inner * 0.82;
    let head_v_y0 = padding + inner * 0.18;
    let head_v_x1 = padding + inner * 0.82;
    let head_v_y1 = padding + inner * 0.55;

    let half = thickness / 2.0;

    for y in 0..size {
        for x in 0..size {
            let px = x as f64 + 0.5;
            let py = y as f64 + 0.5;

            let d_shaft = dist_to_segment(px, py, shaft_x0, shaft_y0, shaft_x1, shaft_y1);
            let d_head_h = dist_to_segment(px, py, head_h_x0, head_h_y0, head_h_x1, head_h_y1);
            let d_head_v = dist_to_segment(px, py, head_v_x0, head_v_y0, head_v_x1, head_v_y1);

            let d_min = d_shaft.min(d_head_h).min(d_head_v);

            // Anti-aliased edge: smooth transition over 1 pixel
            let alpha = (1.0 - (d_min - half + 0.5)).clamp(0.0, 1.0);

            if alpha > 0.0 {
                let a = (alpha * 255.0) as u8;
                let existing = img.get_pixel(x, y);
                // Alpha-blend over existing pixel
                let ea = existing[3] as f64 / 255.0;
                let na = alpha;
                let out_a = na + ea * (1.0 - na);
                if out_a > 0.0 {
                    let r = ((color[0] as f64 * na + existing[0] as f64 * ea * (1.0 - na)) / out_a)
                        as u8;
                    let g = ((color[1] as f64 * na + existing[1] as f64 * ea * (1.0 - na)) / out_a)
                        as u8;
                    let b = ((color[2] as f64 * na + existing[2] as f64 * ea * (1.0 - na)) / out_a)
                        as u8;
                    let _ = a;
                    img.put_pixel(x, y, image::Rgba([r, g, b, (out_a * 255.0) as u8]));
                }
            }
        }
    }
}

/// 32x32 white arrow on transparent — for menu bar tray
fn generate_tray_icon() {
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let icon_path = Path::new(&out_dir).join("tray_icon.png");

    let mut img = image::RgbaImage::new(32, 32);
    draw_arrow_sdf(&mut img, 32, 3.0, 3.5, [255, 255, 255]);
    img.save(&icon_path).expect("Failed to save tray icon");
}

/// 256x256 white arrow on dark rounded-rect background — for dock/taskbar/window
fn generate_app_icon() {
    let size = 256u32;
    let mut img = image::RgbaImage::new(size, size);

    // Draw rounded rectangle background (#18181b — zinc-900)
    let bg = image::Rgba([24, 24, 27, 255]);
    let radius = 48.0f64;
    let pad = 8u32;
    for y in pad..size - pad {
        for x in pad..size - pad {
            let rx = x - pad;
            let ry = y - pad;
            let w = size - 2 * pad;
            let h = size - 2 * pad;
            let in_rect = if rx < radius as u32 && ry < radius as u32 {
                let dx = radius - rx as f64;
                let dy = radius - ry as f64;
                dx * dx + dy * dy <= radius * radius
            } else if rx >= w - radius as u32 && ry < radius as u32 {
                let dx = rx as f64 - (w as f64 - radius);
                let dy = radius - ry as f64;
                dx * dx + dy * dy <= radius * radius
            } else if rx < radius as u32 && ry >= h - radius as u32 {
                let dx = radius - rx as f64;
                let dy = ry as f64 - (h as f64 - radius);
                dx * dx + dy * dy <= radius * radius
            } else if rx >= w - radius as u32 && ry >= h - radius as u32 {
                let dx = rx as f64 - (w as f64 - radius);
                let dy = ry as f64 - (h as f64 - radius);
                dx * dx + dy * dy <= radius * radius
            } else {
                true
            };
            if in_rect {
                img.put_pixel(x, y, bg);
            }
        }
    }

    // Draw white arrow with anti-aliased edges
    draw_arrow_sdf(&mut img, size, 52.0, 22.0, [255, 255, 255]);

    // Save as app icon for Slint window
    let ui_icon_path = Path::new("assets/icon.png");
    img.save(ui_icon_path).expect("Failed to save app icon");

    // Also save to OUT_DIR for macOS dock icon
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let out_icon_path = Path::new(&out_dir).join("app_icon.png");
    img.save(&out_icon_path)
        .expect("Failed to save app icon to OUT_DIR");
}
