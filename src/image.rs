//! 图片模式:直接显示本地图片(kitty 图形协议 / iTerm2 OSC1337)。
//! PNG 直接透传;JPEG/GIF 经 image crate 转码缩放为 PNG 后发送。

use std::io::Write;

pub const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";

/// 文件头嗅探是否为图片(与 Python 版 IMAGE_MAGICS 一致)。
pub fn sniff_image(head: &[u8]) -> bool {
    if head.starts_with(PNG_MAGIC)
        || head.starts_with(b"\xff\xd8\xff")
        || head.starts_with(b"GIF8")
        || head.starts_with(b"BM")
    {
        return true;
    }
    head.len() >= 12 && &head[0..4] == b"RIFF" && &head[8..12] == b"WEBP"
}

/// PNG IHDR 宽高(大端 u32,偏移 16)。
pub fn png_size(png: &[u8]) -> Option<(u32, u32)> {
    if png.len() >= 24 && png.starts_with(PNG_MAGIC) {
        let w = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
        let h = u32::from_be_bytes([png[20], png[21], png[22], png[23]]);
        Some((w, h))
    } else {
        None
    }
}

fn base64_encode(data: &[u8]) -> String {
    const ALPH: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let mut n: u32 = 0;
        for (k, &b) in chunk.iter().enumerate() {
            n |= (b as u32) << (16 - 8 * k);
        }
        out.push(ALPH[((n >> 18) & 63) as usize] as char);
        out.push(ALPH[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            ALPH[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPH[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// 终端字符格像素(失败回退 9x18,与 Python 版一致)。
pub fn cell_pixels() -> (u32, u32) {
    #[cfg(unix)]
    {
        let mut ws = libc::winsize {
            ws_row: 0,
            ws_col: 0,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let ok = unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut ws) } == 0;
        if ok && ws.ws_xpixel > 0 && ws.ws_ypixel > 0 && ws.ws_row > 0 && ws.ws_col > 0 {
            return (
                (ws.ws_xpixel / ws.ws_col).max(1) as u32,
                (ws.ws_ypixel / ws.ws_row).max(1) as u32,
            );
        }
    }
    (9, 18)
}

/// 终端行列数(失败回退 24x80)。
pub fn terminal_cells() -> (u32, u32) {
    #[cfg(unix)]
    {
        let mut ws = libc::winsize {
            ws_row: 0,
            ws_col: 0,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let ok = unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut ws) } == 0;
        if ok && ws.ws_row > 0 && ws.ws_col > 0 {
            return (ws.ws_row as u32, ws.ws_col as u32);
        }
    }
    (24, 80)
}

/// 统一转 PNG(kitty 协议只吃 PNG),超终端 80% 则缩放。
/// 返回 (png字节, 宽, 高)。非 PNG 需要转码,失败返回 Err。
pub fn as_png(data: &[u8]) -> Result<(Vec<u8>, u32, u32), ()> {
    let (cw, chh) = cell_pixels();
    let (rows, cols) = terminal_cells();
    let max_w = (cols * cw) * 8 / 10;
    let max_h = (rows * chh) * 8 / 10;

    if data.starts_with(PNG_MAGIC) {
        if let Some((w, h)) = png_size(data) {
            if w <= max_w && h <= max_h {
                return Ok((data.to_vec(), w, h));
            }
        }
    }

    // JPEG/GIF(/超大 PNG)经 image crate 转码+缩放
    if let Ok(img) = image::load_from_memory(data) {
        let img = img.to_rgba8();
        let (w, h) = (img.width(), img.height());
        let scale = (max_w as f64 / w as f64)
            .min(max_h as f64 / h as f64)
            .min(1.0);
        let img = if scale < 1.0 {
            let nw = ((w as f64 * scale) as u32).max(1);
            let nh = ((h as f64 * scale) as u32).max(1);
            image::imageops::resize(&img, nw, nh, image::imageops::FilterType::Lanczos3)
        } else {
            img
        };
        let mut buf = Vec::new();
        let enc = image::codecs::png::PngEncoder::new(&mut buf);
        use image::ImageEncoder;
        if enc
            .write_image(
                img.as_raw(),
                img.width(),
                img.height(),
                image::ColorType::Rgba8.into(),
            )
            .is_ok()
        {
            if let Some((w, h)) = png_size(&buf) {
                return Ok((buf, w, h));
            }
        }
    }

    // 兜底:PNG 原样(尺寸未知时由终端自行处理)
    if data.starts_with(PNG_MAGIC) {
        if let Some((w, h)) = png_size(data) {
            return Ok((data.to_vec(), w, h));
        }
    }
    Err(())
}

/// kitty 图形协议:PNG base64 分块传输,C=1 固定光标,显示完手动换行。
pub fn send_kitty(out: &mut dyn Write, png: &[u8]) {
    let b64 = base64_encode(png);
    let raw = b64.as_bytes();
    const CHUNK: usize = 4096;
    let mut pieces: Vec<&[u8]> = raw.chunks(CHUNK).collect();
    if pieces.is_empty() {
        pieces.push(b"");
    }
    let last = pieces.len() - 1;
    for (n, piece) in pieces.iter().enumerate() {
        let mut head = b"\x1b_Ga=T,f=24,q=2,C=1".to_vec();
        if pieces.len() > 1 {
            head.extend_from_slice(if n < last { b",m=1" } else { b",m=0" });
        }
        let _ = out.write_all(&head);
        let _ = out.write_all(b";");
        let _ = out.write_all(piece);
        let _ = out.write_all(b"\x1b\\");
    }
    let h = png_size(png).map(|(_, h)| h).unwrap_or(0);
    let (_, chh) = cell_pixels();
    let lines = ((h as f64 / chh as f64).round() as usize).max(1);
    let _ = out.write_all(b"\r");
    for _ in 0..lines {
        let _ = out.write_all(b"\n");
    }
}

/// iTerm2 OSC1337 内联图片(原字节直接发,无需转码)。
pub fn send_iterm(out: &mut dyn Write, data: &[u8], name: &str) {
    let enc_name = base64_encode(name.as_bytes());
    let enc_data = base64_encode(data);
    let _ = writeln!(
        out,
        "\x1b]1337;File=inline=1;size={};preserveAspectRatio=1;name={}:{}\x07",
        data.len(),
        enc_name,
        enc_data
    );
}

/// 图片协议选择:auto 按终端启发式,显式值直接采用。
pub fn pick_proto(want: &str) -> &str {
    if want != "auto" {
        return want;
    }
    let term = std::env::var("TERM").unwrap_or_default();
    let prog = std::env::var("TERM_PROGRAM").unwrap_or_default();
    let env = format!("{term} {prog}").to_lowercase();
    if env.contains("wezterm") || env.contains("iterm") || env.contains("mintty") {
        "iterm"
    } else {
        "kitty"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn png_size_parse() {
        let mut hdr = PNG_MAGIC.to_vec();
        hdr.extend_from_slice(&[0u8; 8]); // length+type
        hdr.extend_from_slice(&1080u32.to_be_bytes());
        hdr.extend_from_slice(&1080u32.to_be_bytes());
        assert_eq!(png_size(&hdr), Some((1080, 1080)));
    }

    #[test]
    fn sniff_magics() {
        assert!(sniff_image(b"\x89PNG\r\n\x1a\nrest"));
        assert!(sniff_image(b"\xff\xd8\xff rest"));
        assert!(sniff_image(b"GIF89a rest"));
        assert!(sniff_image(b"BM rest"));
        assert!(sniff_image(b"RIFFxxxxWEBP"));
        assert!(!sniff_image(b"hello world, this is text"));
    }

    /// BMP/WebP 必须真能解码:sniff_image 认下它们,若 image crate 未启用
    /// 对应特性,kitty 协议下 `as_png` 就会失败(回归测试)。
    #[test]
    fn bmp_and_webp_decode_to_png() {
        let bmp: &[u8] = &[
            0x42, 0x4d, 0x46, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x36, 0x00, 0x00, 0x00, 0x28, 0x00,
            0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x01, 0x00, 0x18, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x10, 0x00, 0x00, 0x00, 0xc4, 0x0e, 0x00, 0x00, 0xc4, 0x0e, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x3c, 0x1e, 0xc8, 0x3c, 0x1e, 0xc8, 0x00, 0x00, 0x3c, 0x1e,
            0xc8, 0x3c, 0x1e, 0xc8, 0x00, 0x00,
        ];
        let webp: &[u8] = &[
            0x52, 0x49, 0x46, 0x46, 0x38, 0x00, 0x00, 0x00, 0x57, 0x45, 0x42, 0x50, 0x56, 0x50, 0x38, 0x20,
            0x2c, 0x00, 0x00, 0x00, 0x90, 0x01, 0x00, 0x9d, 0x01, 0x2a, 0x02, 0x00, 0x02, 0x00, 0x01, 0x40,
            0x26, 0x25, 0xa0, 0x02, 0x74, 0xba, 0x00, 0x03, 0x98, 0x00, 0xfe, 0xf0, 0xfa, 0x2b, 0xae, 0xf1,
            0x69, 0x46, 0xbf, 0xfb, 0xc6, 0x7f, 0xfd, 0xc6, 0x7f, 0xfd, 0xc6, 0x7f, 0xee, 0xe0, 0x00, 0x00,
        ];
        for (name, data) in [("bmp", bmp), ("webp", webp)] {
            assert!(sniff_image(data), "{name} must be sniffed as an image");
            let (png, w, h) = as_png(data).unwrap_or_else(|_| panic!("{name} must decode"));
            assert!(png.starts_with(PNG_MAGIC), "{name} must transcode to PNG");
            assert!(w > 0 && h > 0, "{name} must report dimensions");
        }
    }
}
