//! Decoded raster images.

use std::rc::Rc;

/// An RGBA8 image with straight alpha, ready for the canvas.
#[derive(Clone, Debug, PartialEq)]
pub struct RasterImage {
    pub width: u32,
    pub height: u32,
    /// `width * height * 4` bytes in RGBA order.
    pub pixels: Vec<u8>,
}

impl RasterImage {
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Option<Self> {
        let expected = (width as usize) * (height as usize) * 4;
        (pixels.len() == expected && width > 0 && height > 0).then_some(RasterImage {
            width,
            height,
            pixels,
        })
    }

    /// Decodes PNG, JPEG, GIF or BMP data.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let decoded = image::load_from_memory(bytes).ok()?;
        let rgba = decoded.to_rgba8();
        let (width, height) = rgba.dimensions();
        RasterImage::new(width, height, rgba.into_raw())
    }

    /// A flat single-colour image, used for placeholders and tests.
    pub fn solid(width: u32, height: u32, rgba: [u8; 4]) -> Self {
        RasterImage {
            width,
            height,
            pixels: rgba
                .iter()
                .copied()
                .cycle()
                .take((width as usize) * (height as usize) * 4)
                .collect(),
        }
    }

    pub fn aspect_ratio(&self) -> f32 {
        self.width as f32 / self.height.max(1) as f32
    }
}

/// Supplies decoded images to the display list builder.
pub trait ImageSource {
    fn image(&self, url: &str) -> Option<Rc<RasterImage>>;
}

/// An image source with nothing in it.
pub struct NoImageSource;

impl ImageSource for NoImageSource {
    fn image(&self, _url: &str) -> Option<Rc<RasterImage>> {
        None
    }
}

impl<F> ImageSource for F
where
    F: Fn(&str) -> Option<Rc<RasterImage>>,
{
    fn image(&self, url: &str) -> Option<Rc<RasterImage>> {
        self(url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_mismatched_buffers() {
        assert!(RasterImage::new(2, 2, vec![0; 16]).is_some());
        assert!(RasterImage::new(2, 2, vec![0; 15]).is_none());
        assert!(RasterImage::new(0, 2, vec![]).is_none());
    }

    #[test]
    fn solid_images_are_uniform() {
        let image = RasterImage::solid(3, 2, [1, 2, 3, 4]);
        assert_eq!(image.pixels.len(), 24);
        assert_eq!(&image.pixels[0..4], &[1, 2, 3, 4]);
        assert_eq!(&image.pixels[20..24], &[1, 2, 3, 4]);
        assert_eq!(image.aspect_ratio(), 1.5);
    }

    #[test]
    fn png_round_trip() {
        let original = RasterImage::solid(4, 3, [10, 20, 30, 255]);
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(
            image::RgbaImage::from_raw(4, 3, original.pixels.clone()).unwrap(),
        )
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();

        let decoded = RasterImage::decode(&png.into_inner()).expect("decodable");
        assert_eq!(decoded.width, 4);
        assert_eq!(decoded.height, 3);
        assert_eq!(decoded.pixels, original.pixels);
    }

    #[test]
    fn garbage_does_not_decode() {
        assert!(RasterImage::decode(b"not an image").is_none());
        assert!(RasterImage::decode(&[]).is_none());
    }

    #[test]
    fn closures_can_be_image_sources() {
        let image = Rc::new(RasterImage::solid(1, 1, [0, 0, 0, 255]));
        let source = move |url: &str| (url == "a.png").then(|| image.clone());
        assert!(source.image("a.png").is_some());
        assert!(source.image("b.png").is_none());
        assert!(NoImageSource.image("a.png").is_none());
    }
}
