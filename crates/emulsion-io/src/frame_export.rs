//! Writing rendered frames out: a looping GIF encoder fed one frame at a
//! time (so long animations never hold every picture in memory), letterboxing
//! a picture onto a fixed-size canvas, and a staged GIF file that replaces
//! its target only once complete. Shared by Design motion export, Paint
//! animation and replay GIFs, and storyboard animatic GIFs.
use image::codecs::gif::{GifEncoder, Repeat};
use image::{Delay, ImageResult, Rgba, RgbaImage};
use std::io::Write;
use std::path::Path;

/// A looping GIF being written frame by frame. Dropping it writes the
/// trailer.
pub struct GifFrames<W: Write> {
    encoder: GifEncoder<W>,
}

impl<W: Write> GifFrames<W> {
    /// `speed` is the encoder's colour quantisation speed, 1 (best) to 30.
    pub fn new(writer: W, speed: i32) -> ImageResult<Self> {
        let mut encoder = GifEncoder::new_with_speed(writer, speed);
        encoder.set_repeat(Repeat::Infinite)?;
        Ok(Self { encoder })
    }

    pub fn push(&mut self, image: RgbaImage, delay: Delay) -> ImageResult<()> {
        self.encoder
            .encode_frame(image::Frame::from_parts(image, 0, 0, delay))
    }
}

/// Scale `image` to fit inside `size` (keeping its aspect) and centre it on
/// a canvas of `background`.
pub fn fit(image: &RgbaImage, size: (u32, u32), background: Rgba<u8>) -> RgbaImage {
    let ratio = (size.0 as f64 / image.width() as f64).min(size.1 as f64 / image.height() as f64);
    let image = image::imageops::resize(
        image,
        (image.width() as f64 * ratio).round().max(1.) as u32,
        (image.height() as f64 * ratio).round().max(1.) as u32,
        image::imageops::FilterType::Triangle,
    );
    let mut canvas = RgbaImage::from_pixel(size.0, size.1, background);
    let x = (size.0 - image.width()) / 2;
    let y = (size.1 - image.height()) / 2;
    image::imageops::overlay(&mut canvas, &image, x as i64, y as i64);
    canvas
}

/// Write `frames` as a looping GIF, one frame at a time so a long replay
/// never holds every picture in memory at once. Frames go to a staging file
/// beside `path` that replaces it only once complete, so a failure leaves
/// any existing file untouched.
pub fn encode_gif(
    path: &Path,
    frames: impl IntoIterator<Item = Result<RgbaImage, String>>,
    fps: u32,
) -> Result<(), String> {
    let delay = Delay::from_numer_denom_ms(1000, fps.max(1));
    encode_gif_frames(path, frames.into_iter().map(|f| f.map(|f| (f, delay))), 1)
}

/// [`encode_gif`] with a delay per frame and an encoder `speed` (1–30).
pub fn encode_gif_frames(
    path: &Path,
    frames: impl IntoIterator<Item = Result<(RgbaImage, Delay), String>>,
    speed: i32,
) -> Result<(), String> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let staging = path.with_file_name(format!(".{name}.emulsion-tmp-{}-{seq}", std::process::id()));
    let result = (|| {
        let mut out =
            std::io::BufWriter::new(std::fs::File::create(&staging).map_err(|e| e.to_string())?);
        {
            // Dropping the encoder writes the GIF trailer.
            let mut gif = GifFrames::new(&mut out, speed).map_err(|e| e.to_string())?;
            for f in frames {
                let (image, delay) = f?;
                gif.push(image, delay).map_err(|e| e.to_string())?;
            }
        }
        out.flush().map_err(|e| e.to_string())?;
        out.get_ref().sync_all().map_err(|e| e.to_string())?;
        std::fs::rename(&staging, path).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&staging);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::AnimationDecoder;

    #[test]
    fn frames_keep_their_delays_and_fit_letterboxes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.gif");
        let red = RgbaImage::from_pixel(8, 4, Rgba([255, 0, 0, 255]));
        let boxed = fit(&red, (8, 8), Rgba([255, 255, 255, 255]));
        assert_eq!(boxed.get_pixel(4, 0).0, [255, 255, 255, 255]);
        assert_eq!(boxed.get_pixel(4, 4).0, [255, 0, 0, 255]);
        let frames = [
            Ok((boxed.clone(), Delay::from_numer_denom_ms(100, 1))),
            Ok((boxed, Delay::from_numer_denom_ms(250, 1))),
        ];
        encode_gif_frames(&path, frames, 10).unwrap();
        let decoder = image::codecs::gif::GifDecoder::new(std::io::BufReader::new(
            std::fs::File::open(&path).unwrap(),
        ))
        .unwrap();
        let frames = decoder.into_frames().collect_frames().unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[1].delay().numer_denom_ms(), (250, 1));
        let failed = encode_gif(&path, [Err("render failed".to_string())], 12);
        assert!(failed.is_err());
        assert!(path.exists(), "the earlier file is kept");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
