//! The frosted background, done once. Reading the wallpaper and blurring it here means a card only draws a stretched
//! picture, where live background blur would need the GPU working all the time.

use windows::Win32::Foundation::{ERROR_SUCCESS, GENERIC_READ};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory, WICBitmapDitherTypeNone,
    WICBitmapInterpolationModeFant, WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand, WICRect,
};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};
use windows::Win32::UI::WindowsAndMessaging::{
    SystemParametersInfoW, SPI_GETDESKWALLPAPER, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
};
use windows::core::{w, Result, HSTRING, PCWSTR};

const WIDTH: u32 = 480;
const PASSES: usize = 3; // Three box blurs come close to a Gaussian.
const RADIUS: usize = 9;
const FILL: &str = "10"; // WallpaperStyle for the fit that `read` reproduces.

fn path() -> Option<String> {
    let mut buffer = [0u16; 520];
    unsafe {
        SystemParametersInfoW(
            SPI_GETDESKWALLPAPER,
            buffer.len() as u32,
            Some(buffer.as_mut_ptr().cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
        .ok()?;
    }
    let length = buffer.iter().position(|unit| *unit == 0).unwrap_or(buffer.len());
    (length > 0).then(|| String::from_utf16_lossy(&buffer[..length]))
}

fn desktop_setting(name: PCWSTR) -> Option<String> {
    let mut buffer = [0u16; 16];
    let mut size = (buffer.len() * 2) as u32;
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Control Panel\\Desktop"),
            name,
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    let length = buffer.iter().position(|unit| *unit == 0).unwrap_or(buffer.len());
    (result == ERROR_SUCCESS).then(|| String::from_utf16_lossy(&buffer[..length]))
}

// Any other fit puts a different part of the picture behind each card, and plain cards beat wrong ones.
fn fills_the_screen() -> bool {
    let tiled = desktop_setting(w!("TileWallpaper")).is_some_and(|value| value == "1");
    !tiled && desktop_setting(w!("WallpaperStyle")).is_none_or(|value| value == FILL)
}

// Scaled to fill the screen, like the default wallpaper fit.
fn read(path: &str, screen_width: u32, screen_height: u32) -> Result<(Vec<u8>, u32, u32)> {
    unsafe {
        let factory: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let name = HSTRING::from(path);
        let decoder =
            factory.CreateDecoderFromFilename(PCWSTR(name.as_ptr()), None, GENERIC_READ, WICDecodeMetadataCacheOnDemand)?;
        let frame = decoder.GetFrame(0)?;
        let (mut width, mut height) = (0, 0);
        frame.GetSize(&mut width, &mut height)?;

        let screen = screen_width as f64 / screen_height as f64;
        let picture = width as f64 / height as f64;
        let (crop_width, crop_height) = if picture > screen {
            ((height as f64 * screen) as u32, height)
        } else {
            (width, (width as f64 / screen) as u32)
        };
        let clip = WICRect {
            X: ((width - crop_width) / 2) as i32,
            Y: ((height - crop_height) / 2) as i32,
            Width: crop_width as i32,
            Height: crop_height as i32,
        };
        let clipper = factory.CreateBitmapClipper()?;
        clipper.Initialize(&frame, &clip)?;

        let target_height = (WIDTH as f64 / screen) as u32;
        let scaler = factory.CreateBitmapScaler()?;
        scaler.Initialize(&clipper, WIDTH, target_height, WICBitmapInterpolationModeFant)?;
        let converter = factory.CreateFormatConverter()?;
        converter.Initialize(
            &scaler,
            &GUID_WICPixelFormat32bppPBGRA,
            WICBitmapDitherTypeNone,
            None,
            0.0,
            WICBitmapPaletteTypeCustom,
        )?;
        let mut pixels = vec![0u8; (WIDTH * target_height * 4) as usize];
        converter.CopyPixels(std::ptr::null(), WIDTH * 4, &mut pixels)?;
        Ok((pixels, WIDTH, target_height))
    }
}

// One pass along lines of `count` pixels, `step` bytes apart.
fn box_blur(pixels: &mut [u8], lines: usize, line_step: usize, count: usize, step: usize) {
    let mut source = vec![[0u32; 3]; count];
    for line in 0..lines {
        let start = line * line_step;
        for (index, cell) in source.iter_mut().enumerate() {
            let at = start + index * step;
            *cell = [u32::from(pixels[at]), u32::from(pixels[at + 1]), u32::from(pixels[at + 2])];
        }
        let window = (RADIUS * 2 + 1) as u32;
        let clamp = |index: isize| index.clamp(0, count as isize - 1) as usize;
        let mut sum = [0u32; 3];
        for offset in -(RADIUS as isize)..=RADIUS as isize {
            for channel in 0..3 {
                sum[channel] += source[clamp(offset)][channel];
            }
        }
        for index in 0..count {
            let at = start + index * step;
            for channel in 0..3 {
                pixels[at + channel] = (sum[channel] / window) as u8;
                let leaving = source[clamp(index as isize - RADIUS as isize)][channel];
                let entering = source[clamp(index as isize + RADIUS as isize + 1)][channel];
                sum[channel] = sum[channel] + entering - leaving;
            }
        }
    }
}

fn frost(pixels: &mut [u8], width: usize, height: usize) {
    for _ in 0..PASSES {
        box_blur(pixels, height, width * 4, width, 4);
        box_blur(pixels, width, 4, height, width * 4);
    }
    // Only how light it is comes through: the cards stay monochrome on any wallpaper.
    for pixel in pixels.chunks_exact_mut(4) {
        let level = 0.11 * f32::from(pixel[0]) + 0.59 * f32::from(pixel[1]) + 0.30 * f32::from(pixel[2]);
        pixel[..3].fill(level.round().clamp(0.0, 255.0) as u8);
        pixel[3] = 255;
    }
}

/// The wallpaper as the primary screen shows it, blurred and gray, in BGRA rows. None for a plain colour, a fit other
/// than Fill, or if it can't be read.
pub fn load(screen_width: u32, screen_height: u32) -> Option<(Vec<u8>, u32, u32)> {
    if !fills_the_screen() {
        return None;
    }
    let (mut pixels, width, height) = read(&path()?, screen_width, screen_height).ok()?;
    frost(&mut pixels, width as usize, height as usize);
    Some((pixels, width, height))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blur_keeps_a_flat_picture_flat() {
        let mut pixels = vec![100u8; 64 * 48 * 4];
        frost(&mut pixels, 64, 48);
        assert!(pixels.chunks_exact(4).all(|pixel| pixel[..3] == [100, 100, 100] && pixel[3] == 255));
    }

    #[test]
    fn colour_does_not_come_through() {
        let mut pixels = [0u8, 0, 255, 255].repeat(64 * 48);
        frost(&mut pixels, 64, 48);
        assert!(pixels.chunks_exact(4).all(|pixel| pixel[0] == pixel[1] && pixel[1] == pixel[2]));
    }

    #[test]
    fn blur_spreads_an_edge() {
        let (width, height) = (64usize, 8usize);
        let mut pixels = vec![0u8; width * height * 4];
        for row in pixels.chunks_exact_mut(width * 4) {
            for pixel in row[..width * 2].chunks_exact_mut(4) {
                pixel[..3].fill(200);
            }
        }
        frost(&mut pixels, width, height);
        let middle = pixels[(4 * width + width / 2) * 4];
        assert!(middle > 40 && middle < 160, "{middle}");
    }
}
