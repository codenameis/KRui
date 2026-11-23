//! Lossless RGBA8 Run-Length Encoding (`"KRFR"`) for streaming real `krui-renderer` framebuffers.

/// 4-byte wire header magic identifying a compressed `KRFR` RGBA8 frame.
pub const FRAME_RLE_MAGIC: [u8; 4] = *b"KRFR";

/// Encodes `width x height` tightly packed RGBA8 pixels into `"KRFR"` RLE binary format:
/// `[magic: "KRFR"][seq: u32 LE][width: u16 LE][height: u16 LE][runs: (count_minus_1: u8, r, g, b, a)...]`.
pub fn encode_rle_rgba_frame(seq: u32, width: u16, height: u16, rgba: &[u8]) -> Vec<u8> {
    let pixel_count = (width as usize).saturating_mul(height as usize);
    let expected_bytes = pixel_count.saturating_mul(4);
    let mut out = Vec::with_capacity(12 + (expected_bytes / 4).min(16_384));
    out.extend_from_slice(&FRAME_RLE_MAGIC);
    out.extend_from_slice(&seq.to_le_bytes());
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    if pixel_count == 0 || rgba.len() < expected_bytes {
        return out;
    }
    let slice = &rgba[..expected_bytes];
    let mut idx = 0usize;
    while idx < slice.len() {
        let px = [slice[idx], slice[idx + 1], slice[idx + 2], slice[idx + 3]];
        let mut run = 1usize;
        while run < 256 && idx + run * 4 < slice.len() {
            let off = idx + run * 4;
            if slice[off..off + 4] == px {
                run += 1;
            } else {
                break;
            }
        }
        out.push((run - 1) as u8);
        out.extend_from_slice(&px);
        idx += run * 4;
    }
    out
}

/// Maximum uncompressed RGBA8 frame size accepted by `decode_rle_rgba_frame` (`128 MiB`).
const MAX_DECODED_FRAME_BYTES: usize = 128 * 1024 * 1024;

/// Decodes a `"KRFR"` RLE binary frame back into `(seq, width, height, rgba8)`.
pub fn decode_rle_rgba_frame(bytes: &[u8]) -> Result<(u32, u16, u16, Vec<u8>), String> {
    if bytes.len() < 12 {
        return Err("KRFR frame shorter than 12-byte header".to_string());
    }
    if bytes[..4] != FRAME_RLE_MAGIC {
        return Err("Invalid KRFR magic header".to_string());
    }
    let seq = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    let width = u16::from_le_bytes([bytes[8], bytes[9]]);
    let height = u16::from_le_bytes([bytes[10], bytes[11]]);
    let expected_bytes = (width as usize)
        .saturating_mul(height as usize)
        .saturating_mul(4);
    if expected_bytes > MAX_DECODED_FRAME_BYTES {
        return Err(format!(
            "KRFR frame {width}x{height} ({expected_bytes} bytes) exceeds 128 MiB limit"
        ));
    }
    let body = &bytes[12..];
    if !body.len().is_multiple_of(5) {
        return Err("KRFR RLE body length must be a multiple of 5 bytes".to_string());
    }
    let mut rgba = Vec::with_capacity(expected_bytes.min(body.len().saturating_mul(1024)));
    let (chunks, _) = body.as_chunks::<5>();
    for &chunk in chunks {
        let count = chunk[0] as usize + 1;
        let run_bytes = count.saturating_mul(4);
        if rgba.len().saturating_add(run_bytes) > expected_bytes {
            return Err("KRFR RLE run overflows declared frame dimensions".to_string());
        }
        let px = [chunk[1], chunk[2], chunk[3], chunk[4]];
        for _ in 0..count {
            rgba.extend_from_slice(&px);
        }
    }
    if rgba.len() != expected_bytes {
        return Err(format!(
            "KRFR pixel count mismatch: decoded {} bytes, expected {expected_bytes}",
            rgba.len()
        ));
    }
    Ok((seq, width, height, rgba))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rle_frame_losslessly_round_trips_long_runs_and_mixed_pixels() {
        let mut rgba = Vec::with_capacity(300 * 4);
        for _ in 0..270 {
            rgba.extend_from_slice(&[30, 40, 50, 255]);
        }
        for i in 0..30u8 {
            rgba.extend_from_slice(&[i, i.saturating_mul(2), 200, 255]);
        }
        let encoded = encode_rle_rgba_frame(7, 30, 10, &rgba);
        assert!(encoded.len() < rgba.len());
        let (seq, w, h, decoded) = decode_rle_rgba_frame(&encoded).unwrap();
        assert_eq!((seq, w, h), (7, 30, 10));
        assert_eq!(decoded, rgba);
    }

    #[test]
    fn rle_decoder_rejects_bad_magic() {
        assert!(decode_rle_rgba_frame(b"BAD!00000000").is_err());
    }
}
