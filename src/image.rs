use crate::config::MAX_IMAGE_PIXELS;

const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
const PNG_IHDR_LENGTH: [u8; 4] = 13u32.to_be_bytes();
const JPEG_START: [u8; 2] = [0xff, 0xd8];

pub(crate) fn validate(mime: &str, data: &[u8]) -> bool {
    match mime {
        "image/png" => valid_png(data),
        "image/jpeg" => valid_jpeg(data),
        _ => false,
    }
}

fn valid_png(data: &[u8]) -> bool {
    if data.len() < 33 ||
        !data.starts_with(PNG_SIGNATURE) ||
        data.get(8..12) != Some(&PNG_IHDR_LENGTH) ||
        data.get(12..16) != Some(b"IHDR")
    {
        return false;
    }

    let Some(width) = read_be_u32(data, 16) else {
        return false;
    };
    let Some(height) = read_be_u32(data, 20) else {
        return false;
    };
    valid_dimensions(width, height)
}

fn valid_jpeg(data: &[u8]) -> bool {
    if data.get(..2) != Some(&JPEG_START) {
        return false;
    }

    let mut found_frame = false;
    let mut offset = 2;
    while offset < data.len() {
        if data.get(offset) != Some(&0xff) {
            return false;
        }

        let mut marker_offset = offset + 1;
        while data.get(marker_offset) == Some(&0xff) {
            marker_offset += 1;
        }
        let Some(&marker) = data.get(marker_offset) else {
            return false;
        };
        offset = marker_offset + 1;

        match marker {
            0x01 | 0xd0..=0xd8 => continue,
            0xd9 => return false,
            0xda => return found_frame,
            _ => {},
        }

        let Some(length) = read_be_u16(data, offset) else {
            return false;
        };
        let length = usize::from(length);
        if length < 2 {
            return false;
        }
        let Some(segment_end) = offset.checked_add(length) else {
            return false;
        };
        if segment_end > data.len() {
            return false;
        }

        if is_start_of_frame(marker) {
            if length < 8 {
                return false;
            }
            let Some(height) = read_be_u16(data, offset + 3) else {
                return false;
            };
            let Some(width) = read_be_u16(data, offset + 5) else {
                return false;
            };
            if !valid_dimensions(u32::from(width), u32::from(height)) {
                return false;
            }
            found_frame = true;
        }

        offset = segment_end;
    }
    false
}

const fn is_start_of_frame(marker: u8) -> bool {
    matches!(
        marker,
        0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF
    )
}

fn valid_dimensions(width: u32, height: u32) -> bool {
    width > 0 && height > 0 && u64::from(width) * u64::from(height) <= MAX_IMAGE_PIXELS
}

fn read_be_u32(data: &[u8], offset: usize) -> Option<u32> {
    let bytes: [u8; 4] = data.get(offset..offset + 4)?.try_into().ok()?;
    Some(u32::from_be_bytes(bytes))
}

fn read_be_u16(data: &[u8], offset: usize) -> Option<u16> {
    let bytes: [u8; 2] = data.get(offset..offset + 2)?.try_into().ok()?;
    Some(u16::from_be_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_header(width: u32, height: u32) -> Vec<u8> {
        let mut data = PNG_SIGNATURE.to_vec();
        data.extend_from_slice(&PNG_IHDR_LENGTH);
        data.extend_from_slice(b"IHDR");
        data.extend_from_slice(&width.to_be_bytes());
        data.extend_from_slice(&height.to_be_bytes());
        data.extend_from_slice(&[8, 6, 0, 0, 0]);
        data.extend_from_slice(&[0; 4]);
        data
    }

    fn jpeg_header(width: u16, height: u16) -> Vec<u8> {
        let mut data = vec![0xff, 0xd8, 0xff, 0xc0, 0x00, 0x08, 0x08];
        data.extend_from_slice(&height.to_be_bytes());
        data.extend_from_slice(&width.to_be_bytes());
        data.push(3);
        data.extend_from_slice(&[0xff, 0xda]);
        data
    }

    #[test]
    fn accepts_valid_png_header() {
        assert!(validate("image/png", &png_header(1920, 1080)));
    }

    #[test]
    fn rejects_png_with_oversized_dimensions() {
        assert!(!validate("image/png", &png_header(10_000, 10_000)));
    }

    #[test]
    fn rejects_png_with_bad_signature() {
        let mut data = png_header(10, 10);
        *data.first_mut().expect("PNG header is not empty") = 0;

        assert!(!validate("image/png", &data));
    }

    #[test]
    fn accepts_valid_jpeg_header() {
        assert!(validate("image/jpeg", &jpeg_header(1920, 1080)));
    }

    #[test]
    fn rejects_jpeg_with_oversized_dimensions() {
        assert!(!validate("image/jpeg", &jpeg_header(10_000, 10_000)));
    }

    #[test]
    fn rejects_truncated_jpeg() {
        assert!(!validate("image/jpeg", &[0xff, 0xd8, 0xff, 0xc0]));
    }

    #[test]
    fn rejects_unknown_image_mime() {
        assert!(!validate("image/svg+xml", b"<svg/>"));
    }
}
