use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{self, Read, Write};

pub const MAX_PACKET: usize = 2 * 1024 * 1024;
pub const STATUS: u32 = u32::MAX;
pub const RESOLUTIONS: [&str; 3] = ["640x360", "960x540", "1280x720"];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub resolution: String,
    pub fps: u32,
    pub bitrate_mbps: f64,
    pub preview_fps: u32,
    pub mirror: bool,
    pub steady_fps: bool,
    pub phone_preview: bool,
    pub lens: String,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            resolution: "1280x720".into(),
            fps: 30,
            bitrate_mbps: 3.,
            preview_fps: 30,
            mirror: false,
            steady_fps: true,
            phone_preview: true,
            lens: "rear".into(),
        }
    }
}
impl Settings {
    pub fn validate(&self) -> bool {
        RESOLUTIONS.contains(&self.resolution.as_str())
            && [15, 20, 24, 30].contains(&self.fps)
            && [0, 5, 15, 30].contains(&self.preview_fps)
            && [1., 1.5, 2., 3., 4., 6., 8.].contains(&self.bitrate_mbps)
            && ["rear", "front"].contains(&self.lens.as_str())
    }
    pub fn size(&self) -> (usize, usize) {
        match self.resolution.as_str() {
            "640x360" => (640, 360),
            "960x540" => (960, 540),
            _ => (1280, 720),
        }
    }
    pub fn wire(&self, streaming: bool, revision: u32) -> Value {
        let (w, h) = self.size();
        serde_json::json!({"ok":true,"codec":"h264","remote_control":true,
            "width":w,"height":h,"fps":self.fps,"bitrate":(self.bitrate_mbps*1e6) as u32,
            "steady_fps":self.steady_fps,"phone_preview":self.phone_preview,"lens":self.lens,
            "streaming":streaming,"revision":revision})
    }
}
pub fn read_json(r: &mut impl Read) -> io::Result<Value> {
    let mut bytes = Vec::new();
    for _ in 0..4096 {
        let mut b = [0];
        r.read_exact(&mut b)?;
        if b[0] == b'\n' {
            let value: Value = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
            return if value.is_object() {
                Ok(value)
            } else {
                Err(io::Error::other("Expected object"))
            };
        }
        bytes.push(b[0]);
    }
    Err(io::Error::other("Handshake exceeds 4096 bytes"))
}
pub fn write_json(w: &mut impl Write, value: &Value) -> io::Result<()> {
    serde_json::to_writer(&mut *w, value).map_err(io::Error::other)?;
    w.write_all(b"\n")?;
    w.flush()
}
pub fn read_packet(r: &mut impl Read, data: &mut Vec<u8>) -> io::Result<u32> {
    let mut header = [0; 8];
    r.read_exact(&mut header)?;
    let n = u32::from_be_bytes(header[..4].try_into().unwrap()) as usize;
    let rotation = u32::from_be_bytes(header[4..].try_into().unwrap());
    if !valid_packet(n, rotation) {
        return Err(io::Error::other("Invalid frame header"));
    }
    data.resize(n, 0);
    r.read_exact(data)?;
    Ok(rotation)
}
pub fn valid_packet(n: usize, rotation: u32) -> bool {
    (n == 0 && rotation == 0)
        || (rotation == STATUS && n > 0 && n <= 4096)
        || (n > 0 && n <= MAX_PACKET && [0, 90, 180, 270].contains(&rotation))
}
pub fn write_packet(w: &mut impl Write, data: &[u8], rotation: u32) -> io::Result<()> {
    if !valid_packet(data.len(), rotation) {
        return Err(io::Error::other("Invalid frame"));
    }
    w.write_all(&(data.len() as u32).to_be_bytes())?;
    w.write_all(&rotation.to_be_bytes())?;
    w.write_all(data)?;
    w.flush()
}

/// RGB top-down -> RGBA top-down, clockwise rotation, optional horizontal mirror,
/// aspect-preserving letterbox. Conversion writes directly into the output buffer.
pub fn normalize(
    rgb: &[u8],
    w: usize,
    h: usize,
    rotation: u32,
    mirror: bool,
    target: (usize, usize),
    out: &mut Vec<u8>,
) -> io::Result<()> {
    if w == 0
        || h == 0
        || w > 1280
        || h > 720
        || rgb.len() != w * h * 3
        || ![0, 90, 180, 270].contains(&rotation)
    {
        return Err(io::Error::other("Invalid decoded frame"));
    }
    let (tw, th) = target;
    if tw == 0 || th == 0 || tw > 1280 || th > 720 {
        return Err(io::Error::other("Invalid target"));
    }
    if rotation == 0 && !mirror && (w, h) == target {
        out.resize(w * h * 4, 255);
        for (src, dst) in rgb
            .as_chunks::<3>()
            .0
            .iter()
            .zip(out.as_chunks_mut::<4>().0)
        {
            dst[..3].copy_from_slice(src);
            dst[3] = 255;
        }
        return Ok(());
    }
    let (rw, rh) = if rotation.is_multiple_of(180) {
        (w, h)
    } else {
        (h, w)
    };
    let scale = (tw as f64 / rw as f64).min(th as f64 / rh as f64);
    let dw = ((rw as f64 * scale).round() as usize).max(1).min(tw);
    let dh = ((rh as f64 * scale).round() as usize).max(1).min(th);
    let (ox, oy) = ((tw - dw) / 2, (th - dh) / 2);
    out.resize(tw * th * 4, 0);
    for px in out.as_chunks_mut::<4>().0 {
        px.copy_from_slice(&[13, 19, 29, 255]);
    }
    // Pixel-center bilinear sampling avoids the jagged edges from nearest-neighbor
    // rotation/letterboxing. Precompute the axes and use integer weights per pixel.
    let axis = |source: usize, dest: usize| -> Vec<(usize, usize, u32)> {
        (0..dest)
            .map(|i| {
                let p = ((i as f64 + 0.5) * source as f64 / dest as f64 - 0.5)
                    .clamp(0.0, (source - 1) as f64);
                let low = p.floor() as usize;
                (
                    low,
                    (low + 1).min(source - 1),
                    ((p - low as f64) * 256.).round() as u32,
                )
            })
            .collect()
    };
    let xs = axis(rw, dw);
    let ys = axis(rh, dh);
    let offset = |rx: usize, ry: usize| {
        let rx = if mirror { rw - 1 - rx } else { rx };
        let (sx, sy) = match rotation {
            90 => (ry, h - 1 - rx),
            180 => (w - 1 - rx, h - 1 - ry),
            270 => (w - 1 - ry, rx),
            _ => (rx, ry),
        };
        (sy * w + sx) * 3
    };
    for (y, &(y0, y1, fy)) in ys.iter().enumerate() {
        for (x, &(x0, x1, fx)) in xs.iter().enumerate() {
            let (a, b0, c, d) = (
                offset(x0, y0),
                offset(x1, y0),
                offset(x0, y1),
                offset(x1, y1),
            );
            let b = ((oy + y) * tw + ox + x) * 4;
            for channel in 0..3 {
                let top =
                    u32::from(rgb[a + channel]) * (256 - fx) + u32::from(rgb[b0 + channel]) * fx;
                let bottom =
                    u32::from(rgb[c + channel]) * (256 - fx) + u32::from(rgb[d + channel]) * fx;
                out[b + channel] = ((top * (256 - fy) + bottom * fy + 32768) >> 16) as u8;
            }
        }
    }
    Ok(())
}
/// Shared-memory DirectShow RGB is bottom-up; preview stays top-down.
pub fn bottom_up_copy(src: &[u8], width: usize, dst: &mut [u8]) {
    let row = width * 4;
    for (a, b) in src.chunks_exact(row).rev().zip(dst.chunks_exact_mut(row)) {
        b.copy_from_slice(a);
    }
}
pub fn choose_fps(ranges: &[(i32, i32)], target: i32, steady: bool) -> i32 {
    ranges
        .iter()
        .enumerate()
        .filter(|(_, (lo, hi))| *lo > 0 && hi >= lo)
        .min_by_key(|(_, (lo, hi))| {
            i64::from((target - hi).max(0)) * 1_000_000
                + i64::from((hi - target).abs()) * 1000
                + if steady {
                    i64::from((target - lo).max(0)) * 10
                } else {
                    i64::from(*lo)
                }
        })
        .map(|(i, _)| i as i32)
        .unwrap_or(-1)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frame_bounds() {
        for (n, r) in [(0, 90), (MAX_PACKET + 1, 0), (4097, STATUS), (2, 1)] {
            assert!(!valid_packet(n, r));
        }
        for (n, r) in [(0, 0), (4096, STATUS), (MAX_PACKET, 270)] {
            assert!(valid_packet(n, r));
        }
        let mut bytes = vec![];
        write_packet(&mut bytes, b"abc", 90).unwrap();
        let mut out = vec![];
        assert_eq!(read_packet(&mut &bytes[..], &mut out).unwrap(), 90);
        assert_eq!(out, b"abc");
    }
    #[test]
    fn asymmetric_orientation() {
        // R G / B W. The webcam row order must be B W / R G.
        let rgb = [255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255];
        let mut rgba = vec![];
        normalize(&rgb, 2, 2, 0, false, (2, 2), &mut rgba).unwrap();
        let mut dib = vec![0; 16];
        bottom_up_copy(&rgba, 2, &mut dib);
        assert_eq!(&dib[..4], &[0, 0, 255, 255]);
        assert_eq!(&dib[8..12], &[255, 0, 0, 255]);
        normalize(&rgb, 2, 2, 90, false, (2, 2), &mut rgba).unwrap();
        assert_eq!(&rgba[..4], &[0, 0, 255, 255]);
        normalize(&rgb, 2, 2, 0, true, (2, 2), &mut rgba).unwrap();
        assert_eq!(&rgba[..4], &[0, 255, 0, 255]);
    }
    #[test]
    fn reject_1080p() {
        let s = Settings {
            resolution: "1920x1080".into(),
            ..Default::default()
        };
        assert!(!s.validate());
    }
    #[test]
    fn scaling_interpolates_instead_of_pixelating() {
        let rgb = [0, 0, 0, 255, 255, 255, 0, 0, 0, 255, 255, 255];
        let mut out = vec![];
        normalize(&rgb, 2, 2, 0, false, (4, 4), &mut out).unwrap();
        assert_eq!(
            &out[..16],
            &[0, 0, 0, 255, 64, 64, 64, 255, 191, 191, 191, 255, 255, 255, 255, 255]
        );
    }
    #[test]
    fn steady_fps() {
        assert_eq!(choose_fps(&[(5, 30), (30, 30), (15, 15)], 30, true), 1);
    }
}
