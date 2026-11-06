use std::{collections::VecDeque, path::PathBuf};

use image::{ImageReader, imageops::FilterType};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let source = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("icon.png"));
    let target = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("crates/dopad-app/assets/dopad-icon.png"));
    let source = ImageReader::open(source)?.decode()?.to_rgba8();
    let (width, height) = source.dimensions();
    let mut pixels = source;
    let mut seen = vec![false; (width as usize).saturating_mul(height as usize)];
    let mut queue = VecDeque::new();

    for x in 0..width {
        queue.push_back((x, 0));
        queue.push_back((x, height - 1));
    }
    for y in 0..height {
        queue.push_back((0, y));
        queue.push_back((width - 1, y));
    }

    while let Some((x, y)) = queue.pop_front() {
        let index = (y as usize) * (width as usize) + x as usize;
        if seen[index] {
            continue;
        }
        seen[index] = true;
        let pixel = pixels.get_pixel(x, y);
        if pixel[3] == 0 || pixel[0].max(pixel[1]).max(pixel[2]) > 16 {
            continue;
        }
        pixels.get_pixel_mut(x, y)[3] = 0;
        if x > 0 {
            queue.push_back((x - 1, y));
        }
        if x + 1 < width {
            queue.push_back((x + 1, y));
        }
        if y > 0 {
            queue.push_back((x, y - 1));
        }
        if y + 1 < height {
            queue.push_back((x, y + 1));
        }
    }

    let resized = image::imageops::resize(&pixels, 512, 512, FilterType::Lanczos3);
    resized.save(target)?;
    Ok(())
}
