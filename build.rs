use std::path::Path;

fn main() {
    // Generate icon BEFORE compiling Slint (Slint references ui/icon.png)
    generate_icon();

    slint_build::compile("ui/main.slint").unwrap();
    println!("cargo::rerun-if-changed=build.rs");
}

fn generate_icon() {
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let icon_path = Path::new(&out_dir).join("tray_icon.png");

    let mut img = image::RgbaImage::new(32, 32);

    // Draw a thick arrow pointing top-right
    // Arrow body: diagonal line from bottom-left to top-right (thick)
    for i in 0..22 {
        for t in -2i32..=2 {
            let x = (6 + i + t).clamp(0, 31) as u32;
            let y = (25 - i + t).clamp(0, 31) as u32;
            img.put_pixel(x, y, image::Rgba([255, 255, 255, 255]));
            // Extra thickness
            let x2 = (6 + i + t).clamp(0, 31) as u32;
            let y2 = (25 - i).clamp(0, 31) as u32;
            img.put_pixel(x2, y2, image::Rgba([255, 255, 255, 255]));
        }
    }
    // Arrowhead: horizontal bar at top-right
    for x in 18..28u32 {
        for t in 0..3u32 {
            let y = (4 + t).min(31);
            img.put_pixel(x, y, image::Rgba([255, 255, 255, 255]));
        }
    }
    // Arrowhead: vertical bar at top-right
    for y in 4..14u32 {
        for t in 0..3u32 {
            let x = (25 + t).min(31);
            img.put_pixel(x, y, image::Rgba([255, 255, 255, 255]));
        }
    }

    img.save(&icon_path).expect("Failed to save tray icon");

    // Also save to ui/ so Slint can reference it as window icon
    let ui_icon_path = Path::new("ui/icon.png");
    img.save(ui_icon_path).expect("Failed to save UI icon");
}
