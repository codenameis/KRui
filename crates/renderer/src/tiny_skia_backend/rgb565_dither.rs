//! Zero-allocation premultiplied RGBA8 -> RGB565 little-endian conversion with optional 4x4 Bayer
//! ordered dithering for 16-bit SPI TFT kiosk panels (`hx8357d` / `krui-kms`).

use krui_style::DamageRect;

const BAYER_4X4: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

/// Converts `region` inside a premultiplied RGBA8 framebuffer into little-endian RGB565 bytes in
/// `out_rgb565_le` (`region.width * region.height * 2` bytes minimum). Zero heap allocations.
pub fn convert_rgba8_to_rgb565_le_dithered(
    rgba_pm: &[u8],
    fb_width: u32,
    fb_height: u32,
    region: DamageRect,
    enable_bayer_dither: bool,
    out_rgb565_le: &mut [u8],
) -> usize {
    let rect = region.clamp_to(fb_width, fb_height);
    if rect.is_empty() {
        return 0;
    }
    let px_count = (rect.width as usize).saturating_mul(rect.height as usize);
    let required_bytes = px_count.saturating_mul(2);
    if out_rgb565_le.len() < required_bytes {
        return 0;
    }
    let fb_stride = fb_width as usize * 4;
    let mut dst_idx = 0_usize;

    for y in rect.y..rect.bottom() {
        let row_bayer = &BAYER_4X4[(y as usize) & 3];
        let row_off = y as usize * fb_stride + rect.x as usize * 4;
        for col in 0..rect.width as usize {
            let x = rect.x as usize + col;
            let src_off = row_off + col * 4;
            let r = rgba_pm[src_off];
            let g = rgba_pm[src_off + 1];
            let b = rgba_pm[src_off + 2];
            let (r8, g8, b8) = (r, g, b);
            let (r5, g6, b5) = if enable_bayer_dither {
                let d16 = row_bayer[x & 3] as u16;
                let rb = (((r8 as u16).saturating_add(d16 >> 1)).min(255)) >> 3;
                let gb = (((g8 as u16).saturating_add(d16 >> 2)).min(255)) >> 2;
                let bb = (((b8 as u16).saturating_add(d16 >> 1)).min(255)) >> 3;
                (rb, gb, bb)
            } else {
                ((r8 as u16) >> 3, (g8 as u16) >> 2, (b8 as u16) >> 3)
            };
            let px: u16 = (r5 << 11) | (g6 << 5) | b5;
            out_rgb565_le[dst_idx] = (px & 0xff) as u8;
            out_rgb565_le[dst_idx + 1] = (px >> 8) as u8;
            dst_idx += 2;
        }
    }
    dst_idx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb565_preserves_solid_primaries_and_reduces_gradient_banding_with_dither() {
        let fb = [
            0, 0, 0, 255, 255, 255, 255, 255, 255, 0, 0, 255, 0, 255, 0, 255,
        ];
        let mut out = [0_u8; 8];
        let n = convert_rgba8_to_rgb565_le_dithered(
            &fb,
            4,
            1,
            DamageRect::new(0, 0, 4, 1),
            true,
            &mut out,
        );
        assert_eq!(n, 8);
        assert_eq!(u16::from_le_bytes([out[0], out[1]]), 0x0000);
        assert_eq!(u16::from_le_bytes([out[2], out[3]]), 0xFFFF);
        assert_eq!(u16::from_le_bytes([out[4], out[5]]), 0xF800);
        assert_eq!(u16::from_le_bytes([out[6], out[7]]), 0x07E0);

        let mut grad = Vec::with_capacity(64 * 4);
        for x in 0..64_u8 {
            let v = 120 + (x / 16);
            grad.extend_from_slice(&[v, v, v, 255]);
        }
        let mut out_dither = vec![0_u8; 128];
        convert_rgba8_to_rgb565_le_dithered(
            &grad,
            64,
            1,
            DamageRect::new(0, 0, 64, 1),
            true,
            &mut out_dither,
        );
        let unique: std::collections::HashSet<u16> = out_dither
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&c| u16::from_le_bytes(c))
            .collect();
        assert!(unique.len() >= 2);
    }
}
