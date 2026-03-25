use std::path::Path;

fn main() {
    // Generate icons BEFORE compiling Slint (Slint references ui/icon.png)
    generate_tray_icon();
    generate_app_icon();

    slint_build::compile("ui/main.slint").unwrap();
    println!("cargo::rerun-if-changed=build.rs");
}

/// 32x32 white arrow on transparent — for menu bar tray
fn generate_tray_icon() {
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let icon_path = Path::new(&out_dir).join("tray_icon.png");

    let mut img = image::RgbaImage::new(32, 32);
    draw_arrow(&mut img, 32, image::Rgba([255, 255, 255, 255]));
    img.save(&icon_path).expect("Failed to save tray icon");
}

/// 256x256 white arrow on dark rounded-rect background — for dock/taskbar/window
fn generate_app_icon() {
    let size = 256u32;
    let mut img = image::RgbaImage::new(size, size);

    // Draw rounded rectangle background (#18181b — zinc-900)
    let bg = image::Rgba([24, 24, 27, 255]);
    let radius = 48.0f64;
    let pad = 8u32; // padding from edge
    for y in pad..size - pad {
        for x in pad..size - pad {
            let rx = x - pad;
            let ry = y - pad;
            let w = size - 2 * pad;
            let h = size - 2 * pad;
            // Check if inside rounded rect
            let in_rect = if rx < radius as u32 && ry < radius as u32 {
                // top-left corner
                let dx = radius - rx as f64;
                let dy = radius - ry as f64;
                dx * dx + dy * dy <= radius * radius
            } else if rx >= w - radius as u32 && ry < radius as u32 {
                // top-right corner
                let dx = rx as f64 - (w as f64 - radius);
                let dy = radius - ry as f64;
                dx * dx + dy * dy <= radius * radius
            } else if rx < radius as u32 && ry >= h - radius as u32 {
                // bottom-left corner
                let dx = radius - rx as f64;
                let dy = ry as f64 - (h as f64 - radius);
                dx * dx + dy * dy <= radius * radius
            } else if rx >= w - radius as u32 && ry >= h - radius as u32 {
                // bottom-right corner
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

    // Draw white arrow scaled to 256x256 (with padding)
    draw_arrow_scaled(&mut img, size, 56, image::Rgba([255, 255, 255, 255]));

    // Save as app icon for Slint window
    let ui_icon_path = Path::new("ui/icon.png");
    img.save(ui_icon_path).expect("Failed to save app icon");

    // Also save to OUT_DIR for potential future use
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let out_icon_path = Path::new(&out_dir).join("app_icon.png");
    img.save(&out_icon_path).expect("Failed to save app icon to OUT_DIR");
}

/// Draw the arrow at native 32x32 scale
fn draw_arrow(img: &mut image::RgbaImage, size: u32, color: image::Rgba<u8>) {
    let _ = size; // arrow coordinates are for 32x32
    // Arrow body: diagonal line from bottom-left to top-right (thick)
    for i in 0..22 {
        for t in -2i32..=2 {
            let x = (6 + i + t).clamp(0, 31) as u32;
            let y = (25 - i + t).clamp(0, 31) as u32;
            img.put_pixel(x, y, color);
            let x2 = (6 + i + t).clamp(0, 31) as u32;
            let y2 = (25 - i).clamp(0, 31) as u32;
            img.put_pixel(x2, y2, color);
        }
    }
    // Arrowhead: horizontal bar at top-right
    for x in 18..28u32 {
        for t in 0..3u32 {
            let y = (4 + t).min(31);
            img.put_pixel(x, y, color);
        }
    }
    // Arrowhead: vertical bar at top-right
    for y in 4..14u32 {
        for t in 0..3u32 {
            let x = (25 + t).min(31);
            img.put_pixel(x, y, color);
        }
    }
}

/// Draw a scaled arrow with padding inside a larger image
fn draw_arrow_scaled(img: &mut image::RgbaImage, size: u32, padding: u32, color: image::Rgba<u8>) {
    let draw_size = size - 2 * padding;
    let scale = draw_size as f64 / 32.0;
    let thickness = (3.0 * scale) as i32;

    // Arrow body: diagonal from bottom-left to top-right
    for i in 0..(22.0 * scale) as i32 {
        for t in -(thickness)..=thickness {
            let bx = ((6.0 * scale) as i32 + i + t).clamp(0, draw_size as i32 - 1) as u32 + padding;
            let by = ((25.0 * scale) as i32 - i + t).clamp(0, draw_size as i32 - 1) as u32 + padding;
            img.put_pixel(bx, by, color);
            let by2 = ((25.0 * scale) as i32 - i).clamp(0, draw_size as i32 - 1) as u32 + padding;
            img.put_pixel(bx, by2, color);
        }
    }
    // Arrowhead: horizontal bar
    let bar_thickness = (3.0 * scale) as u32;
    for x in (18.0 * scale) as u32..(28.0 * scale) as u32 {
        for t in 0..bar_thickness {
            let px = (x + padding).min(size - 1);
            let py = ((4.0 * scale) as u32 + t + padding).min(size - 1);
            img.put_pixel(px, py, color);
        }
    }
    // Arrowhead: vertical bar
    for y in (4.0 * scale) as u32..(14.0 * scale) as u32 {
        for t in 0..bar_thickness {
            let px = ((25.0 * scale) as u32 + t + padding).min(size - 1);
            let py = (y + padding).min(size - 1);
            img.put_pixel(px, py, color);
        }
    }
}
