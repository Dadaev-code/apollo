use bytes::Bytes;
use color_eyre::Result;
use jpeg_decoder::Decoder;

use super::frame::PixelFormat;

pub fn decode_frame(data: &[u8], format: PixelFormat) -> Result<Vec<u8>> {
    match format {
        PixelFormat::Mjpeg => {
            let mut decoder = Decoder::new(data);
            let pixels = decoder.decode()?;
            Ok(pixels)
        }
        PixelFormat::Rgb24 => {
            // Already in RGB format
            Ok(data.to_vec())
        }
        PixelFormat::Yuyv4 => {
            // Convert YUYV to RGB (implement conversion)
            todo!("YUYV to RGB conversion not implemented")
        }
        _ => {
            todo!("Unsupported format: {:?}", format)
        }
    }
}
