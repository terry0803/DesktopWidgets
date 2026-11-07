//! Drawing. Direct2D's software renderer into one shared bitmap: Windows keeps a copy of each layered window's
//! picture, so nothing here has to stay around between updates, and no GPU device is ever created.

use std::cell::RefCell;
use std::collections::HashMap;
use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Foundation::{COLORREF, HWND, POINT, RECT, SIZE};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_IGNORE, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_BEZIER_SEGMENT, D2D1_COLOR_F, D2D1_FIGURE_BEGIN_FILLED,
    D2D1_FIGURE_END_CLOSED, D2D1_PIXEL_FORMAT, D2D_POINT_2F, D2D_RECT_F, D2D_SIZE_F, D2D_SIZE_U,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Bitmap, ID2D1BitmapBrush, ID2D1Brush, ID2D1DCRenderTarget, ID2D1Factory, ID2D1PathGeometry,
    ID2D1SolidColorBrush, ID2D1StrokeStyle, D2D1_ANTIALIAS_MODE_ALIASED, D2D1_ARC_SEGMENT, D2D1_ARC_SIZE_LARGE,
    D2D1_ARC_SIZE_SMALL, D2D1_BITMAP_BRUSH_PROPERTIES, D2D1_SWEEP_DIRECTION_CLOCKWISE,
    D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE,
    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_BITMAP_PROPERTIES, D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_ELLIPSE,
    D2D1_EXTEND_MODE_CLAMP, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT,
    D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_SOFTWARE, D2D1_RENDER_TARGET_USAGE_NONE, D2D1_ROUNDED_RECT,
    D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL,
    DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_MEDIUM, DWRITE_FONT_WEIGHT_SEMI_LIGHT,
    DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_MEASURING_MODE_NATURAL, DWRITE_PIXEL_GEOMETRY_FLAT,
    DWRITE_RENDERING_MODE_NATURAL_SYMMETRIC,
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING,
    DWRITE_TEXT_ALIGNMENT_TRAILING, DWRITE_TRIMMING, DWRITE_TRIMMING_GRANULARITY_CHARACTER,
    DWRITE_WORD_WRAPPING_NO_WRAP,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ,
};
use windows::Win32::UI::WindowsAndMessaging::{UpdateLayeredWindow, ULW_ALPHA};
use windows::core::{Result, HSTRING, PCWSTR};

pub type Color = D2D1_COLOR_F;

pub const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    Color { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a }
}

/// Monochrome, like the Mac's always-dimmed widgets: hierarchy comes from three levels of gray, with no colour at all.
#[derive(Clone, Copy)]
pub struct Palette {
    /// Over the blurred wallpaper. Without a wallpaper `solid` is used instead.
    pub tint: Color,
    pub solid: Color,
    pub edge: Color,
    pub primary: Color,
    pub secondary: Color,
    pub tertiary: Color,
    pub track: Color,
    pub tile: Color,
    pub leaf: Color,
    pub hinge: Color,
    pub accent: Color,
    pub on_accent: Color,
    pub bar: Color,
    pub sun: Color,
    pub rain: Color,
    pub cloud: Color,
}

pub const DARK: Palette = Palette {
    tint: rgba(22, 22, 25, 0.66),
    solid: rgba(26, 26, 29, 1.0),
    edge: rgba(255, 255, 255, 0.08),
    primary: rgba(255, 255, 255, 0.95),
    secondary: rgba(255, 255, 255, 0.64),
    tertiary: rgba(255, 255, 255, 0.42),
    track: rgba(255, 255, 255, 0.16),
    tile: rgba(255, 255, 255, 0.07),
    leaf: rgba(44, 44, 48, 1.0),
    hinge: rgba(0, 0, 0, 0.30),
    accent: rgba(255, 255, 255, 0.92),
    on_accent: rgba(24, 24, 26, 0.96),
    bar: rgba(255, 255, 255, 0.78),
    sun: rgba(255, 255, 255, 0.95),
    rain: rgba(255, 255, 255, 0.72),
    cloud: rgba(190, 190, 198, 1.0),
};

pub const LIGHT: Palette = Palette {
    tint: rgba(250, 250, 252, 0.68),
    solid: rgba(246, 246, 248, 1.0),
    edge: rgba(255, 255, 255, 0.45),
    primary: rgba(0, 0, 0, 0.80),
    secondary: rgba(60, 60, 67, 0.56),
    tertiary: rgba(60, 60, 67, 0.34),
    track: rgba(60, 60, 67, 0.12),
    tile: rgba(255, 255, 255, 0.46),
    leaf: rgba(244, 244, 246, 1.0),
    hinge: rgba(0, 0, 0, 0.10),
    accent: rgba(0, 0, 0, 0.62),
    on_accent: rgba(255, 255, 255, 0.98),
    bar: rgba(0, 0, 0, 0.48),
    sun: rgba(0, 0, 0, 0.66),
    rain: rgba(60, 60, 67, 0.62),
    cloud: rgba(142, 142, 150, 1.0),
};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Align {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Weight {
    SemiLight,
    Regular,
    Medium,
    Semibold,
}

impl Weight {
    fn value(self) -> DWRITE_FONT_WEIGHT {
        match self {
            Weight::SemiLight => DWRITE_FONT_WEIGHT_SEMI_LIGHT,
            Weight::Regular => DWRITE_FONT_WEIGHT_NORMAL,
            Weight::Medium => DWRITE_FONT_WEIGHT_MEDIUM,
            Weight::Semibold => DWRITE_FONT_WEIGHT_SEMI_BOLD,
        }
    }
}

// Closest here to Apple's PingFang, and unlike Microsoft JhengHei it has a medium weight. Windows falls back if it's missing.
const TEXT_FONT: &str = "Noto Sans TC";
// Like SF Text and SF Display: a cut for small sizes, and a finer one from 20 points up.
const NUMBER_FONT: &str = "Segoe UI Variable Text";
const LARGE_NUMBER_FONT: &str = "Segoe UI Variable Display";

struct Surface {
    dc: HDC,
    bitmap: HBITMAP,
    original: HGDIOBJ,
    bits: *mut u8,
    width: i32,
    height: i32,
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            if !self.bitmap.is_invalid() {
                SelectObject(self.dc, self.original);
                let _ = DeleteObject(self.bitmap.into());
            }
            let _ = DeleteDC(self.dc);
        }
    }
}

/// Which part of the screen a card covers, as fractions, for the part of the wallpaper behind it.
#[derive(Clone, Copy)]
pub struct Backdrop {
    pub left: f32,
    pub top: f32,
    pub screen_width: f32,
}

struct Wallpaper {
    brush: ID2D1BitmapBrush,
    _bitmap: ID2D1Bitmap,
    width: f32,
    height: f32,
}

pub struct Gfx {
    factory: ID2D1Factory,
    target: ID2D1DCRenderTarget,
    brush: ID2D1SolidColorBrush,
    write: IDWriteFactory,
    fonts: RefCell<HashMap<(u32, Weight, bool, Align), IDWriteTextFormat>>,
    surface: Surface,
    wallpaper: Option<Wallpaper>,
    pub backdrop: Option<Backdrop>,
    pub palette: Palette,
}

impl Gfx {
    pub fn new() -> Result<Self> {
        unsafe {
            let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let properties = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.0,
                dpiY: 96.0,
                usage: D2D1_RENDER_TARGET_USAGE_NONE,
                minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
            };
            let target = factory.CreateDCRenderTarget(&properties)?;
            target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE); // ClearType needs an opaque background.
            let write: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            // Without the default contrast boost: it thickens strokes, which reads as heavy for light text on dark.
            let params = write.CreateCustomRenderingParams(
                1.8,
                0.0,
                0.0,
                DWRITE_PIXEL_GEOMETRY_FLAT,
                DWRITE_RENDERING_MODE_NATURAL_SYMMETRIC,
            )?;
            target.SetTextRenderingParams(&params);
            let brush = target.CreateSolidColorBrush(&DARK.primary, None)?;
            let dc = CreateCompatibleDC(None);
            if dc.is_invalid() {
                return Err(windows::core::Error::from_win32());
            }
            Ok(Self {
                factory,
                target,
                brush,
                write,
                fonts: RefCell::new(HashMap::new()),
                surface: Surface {
                    dc,
                    bitmap: HBITMAP::default(),
                    original: HGDIOBJ::default(),
                    bits: std::ptr::null_mut(),
                    width: 0,
                    height: 0,
                },
                wallpaper: None,
                backdrop: None,
                palette: DARK,
            })
        }
    }

    /// Takes the already blurred wallpaper as BGRA rows, or None to go back to plain cards.
    pub fn set_wallpaper(&mut self, picture: Option<(Vec<u8>, u32, u32)>) {
        self.wallpaper = picture.and_then(|(pixels, width, height)| unsafe {
            let properties = D2D1_BITMAP_PROPERTIES {
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_IGNORE },
                dpiX: 96.0,
                dpiY: 96.0,
            };
            let size = D2D_SIZE_U { width, height };
            let bitmap = self.target.CreateBitmap(size, Some(pixels.as_ptr().cast()), width * 4, &properties).ok()?;
            let brush_properties = D2D1_BITMAP_BRUSH_PROPERTIES {
                extendModeX: D2D1_EXTEND_MODE_CLAMP,
                extendModeY: D2D1_EXTEND_MODE_CLAMP,
                interpolationMode: D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
            };
            let brush = self.target.CreateBitmapBrush(&bitmap, Some(&brush_properties), None).ok()?;
            Some(Wallpaper { brush, _bitmap: bitmap, width: width as f32, height: height as f32 })
        });
    }

    fn ensure_surface(&mut self, width: i32, height: i32) -> Result<()> {
        let surface = &mut self.surface;
        if surface.width >= width && surface.height >= height {
            return Ok(());
        }
        let (width, height) = (width.max(surface.width), height.max(surface.height));
        unsafe {
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height, // Top-down.
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            let bitmap = CreateDIBSection(Some(surface.dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)?;
            let previous = SelectObject(surface.dc, bitmap.into());
            if surface.bitmap.is_invalid() {
                surface.original = previous;
            } else {
                let _ = DeleteObject(surface.bitmap.into());
            }
            surface.bitmap = bitmap;
            surface.bits = bits.cast();
            surface.width = width;
            surface.height = height;
        }
        Ok(())
    }

    /// Runs `draw` in device-independent pixels on a cleared picture of the given size.
    pub fn paint(&mut self, width: i32, height: i32, scale: f32, draw: impl FnOnce(&Gfx)) -> Result<()> {
        self.ensure_surface(width, height)?;
        unsafe {
            let bounds = RECT { left: 0, top: 0, right: width, bottom: height };
            self.target.BindDC(self.surface.dc, &bounds)?;
            self.target.SetDpi(96.0 * scale, 96.0 * scale);
            self.target.BeginDraw();
            self.target.SetTransform(&Matrix3x2::identity());
            self.target.Clear(Some(&rgba(0, 0, 0, 0.0)));
            draw(self);
            self.target.EndDraw(None, None)
        }
    }

    /// Hands the last painted picture to a window. Windows keeps it, so the shared bitmap can be reused at once.
    /// With a position the window goes there in the same step, so it's never seen with the wrong picture.
    pub fn present(&self, window: HWND, position: Option<POINT>, width: i32, height: i32) -> Result<()> {
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let size = SIZE { cx: width, cy: height };
        let source = POINT { x: 0, y: 0 };
        unsafe {
            UpdateLayeredWindow(
                window,
                None,
                position.as_ref().map(std::ptr::from_ref),
                Some(&size),
                Some(self.surface.dc),
                Some(&source),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            )
        }
    }

    /// The last painted picture over a flat colour, as bottom-up BGR rows, for checking cards without a desktop.
    pub fn flattened(&self, width: i32, height: i32, background: [u8; 3]) -> Vec<u8> {
        let stride = (width as usize * 3).div_ceil(4) * 4;
        let mut rows = vec![0u8; stride * height as usize];
        for y in 0..height as usize {
            for x in 0..width as usize {
                let source = (y * self.surface.width as usize + x) * 4;
                let pixel = unsafe { std::slice::from_raw_parts(self.surface.bits.add(source), 4) };
                let target = (height as usize - 1 - y) * stride + x * 3;
                let keep = 255 - u32::from(pixel[3]);
                for channel in 0..3 {
                    let under = u32::from(background[channel]) * keep / 255;
                    rows[target + channel] = (u32::from(pixel[channel]) + under).min(255) as u8;
                }
            }
        }
        rows
    }

    /// The average colour of a row of the last painted picture, as RGB.
    pub fn sample(&self, left: i32, right: i32, y: i32) -> [u8; 3] {
        if self.surface.bits.is_null() {
            return [0; 3];
        }
        let row = y.clamp(0, self.surface.height - 1) as usize;
        let (left, right) = (left.max(0), right.min(self.surface.width - 1).max(left.max(0)));
        let mut sum = [0u32; 3];
        for x in left..=right {
            let at = (row * self.surface.width as usize + x as usize) * 4;
            let pixel = unsafe { std::slice::from_raw_parts(self.surface.bits.add(at), 3) };
            for channel in 0..3 {
                sum[channel] += u32::from(pixel[2 - channel]);
            }
        }
        sum.map(|total| (total / (right - left + 1) as u32) as u8)
    }

    /// A part of the last painted picture, as BGRA rows.
    pub fn region(&self, x: i32, y: i32, width: i32, height: i32) -> Vec<u8> {
        if self.surface.bits.is_null() {
            return vec![0; (width * height * 4) as usize];
        }
        let mut rows = Vec::with_capacity((width * height * 4) as usize);
        for row in y..y + height {
            let row = row.clamp(0, self.surface.height - 1) as usize;
            let left = x.clamp(0, self.surface.width - 1) as usize;
            let length = (width as usize).min(self.surface.width as usize - left) * 4;
            let start = (row * self.surface.width as usize + left) * 4;
            rows.extend_from_slice(unsafe { std::slice::from_raw_parts(self.surface.bits.add(start), length) });
            rows.resize(rows.len() + width as usize * 4 - length, 0);
        }
        rows
    }

    fn font(&self, size: f32, weight: Weight, number: bool, align: Align) -> IDWriteTextFormat {
        let key = ((size * 4.0) as u32, weight, number, align);
        self.fonts
            .borrow_mut()
            .entry(key)
            .or_insert_with(|| unsafe {
                let name = match (number, size >= 20.0) {
                    (false, _) => TEXT_FONT,
                    (true, false) => NUMBER_FONT,
                    (true, true) => LARGE_NUMBER_FONT,
                };
                let family = HSTRING::from(name);
                let locale = HSTRING::from("zh-TW");
                let format = self
                    .write
                    .CreateTextFormat(
                        PCWSTR(family.as_ptr()),
                        None,
                        weight.value(),
                        DWRITE_FONT_STYLE_NORMAL,
                        DWRITE_FONT_STRETCH_NORMAL,
                        size,
                        PCWSTR(locale.as_ptr()),
                    )
                    .expect("text format");
                let _ = format.SetTextAlignment(match align {
                    Align::Left => DWRITE_TEXT_ALIGNMENT_LEADING,
                    Align::Center => DWRITE_TEXT_ALIGNMENT_CENTER,
                    Align::Right => DWRITE_TEXT_ALIGNMENT_TRAILING,
                });
                let _ = format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
                let _ = format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
                if let Ok(sign) = self.write.CreateEllipsisTrimmingSign(&format) {
                    let trimming = DWRITE_TRIMMING {
                        granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                        delimiter: 0,
                        delimiterCount: 0,
                    };
                    let _ = format.SetTrimming(&trimming, &sign);
                }
                format
            })
            .clone()
    }

    fn set(&self, color: Color) {
        unsafe { self.brush.SetColor(&color) }
    }

    // Apple's continuous corners: the curve starts further along the edge and eases in, where a plain rounded
    // rectangle meets the straight part abruptly.
    fn squircle(&self, x: f32, y: f32, w: f32, h: f32, radius: f32) -> Option<ID2D1PathGeometry> {
        let reach = (radius * 1.45).min(w / 2.0).min(h / 2.0);
        let hold = reach * 0.28;
        let (right, bottom) = (x + w, y + h);
        let point = |x: f32, y: f32| D2D_POINT_2F { x, y };
        let corner = |a: D2D_POINT_2F, b: D2D_POINT_2F, c: D2D_POINT_2F| D2D1_BEZIER_SEGMENT { point1: a, point2: b, point3: c };
        unsafe {
            let path = self.factory.CreatePathGeometry().ok()?;
            let sink = path.Open().ok()?;
            sink.BeginFigure(point(x + reach, y), D2D1_FIGURE_BEGIN_FILLED);
            sink.AddLine(point(right - reach, y));
            sink.AddBezier(&corner(point(right - hold, y), point(right, y + hold), point(right, y + reach)));
            sink.AddLine(point(right, bottom - reach));
            sink.AddBezier(&corner(point(right, bottom - hold), point(right - hold, bottom), point(right - reach, bottom)));
            sink.AddLine(point(x + reach, bottom));
            sink.AddBezier(&corner(point(x + hold, bottom), point(x, bottom - hold), point(x, bottom - reach)));
            sink.AddLine(point(x, y + reach));
            sink.AddBezier(&corner(point(x, y + hold), point(x + hold, y), point(x + reach, y)));
            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
            sink.Close().ok()?;
            Some(path)
        }
    }

    /// The card itself: the blurred wallpaper behind it under a tint, with a hairline edge.
    pub fn card(&self, w: f32, h: f32, radius: f32) {
        let Some(shape) = self.squircle(0.5, 0.5, w - 1.0, h - 1.0, radius) else { return };
        let palette = self.palette;
        unsafe {
            match (&self.wallpaper, self.backdrop) {
                (Some(wallpaper), Some(view)) => {
                    let pixels_per_dip = wallpaper.width / view.screen_width;
                    let to_card = Matrix3x2 {
                        M11: 1.0 / pixels_per_dip,
                        M12: 0.0,
                        M21: 0.0,
                        M22: 1.0 / pixels_per_dip,
                        M31: -view.left * wallpaper.width / pixels_per_dip,
                        M32: -view.top * wallpaper.height / pixels_per_dip,
                    };
                    wallpaper.brush.SetTransform(&to_card);
                    self.target.FillGeometry(&shape, &wallpaper.brush, None::<&ID2D1Brush>);
                    self.set(palette.tint);
                }
                _ => self.set(palette.solid),
            }
            self.target.FillGeometry(&shape, &self.brush, None::<&ID2D1Brush>);
            self.set(palette.edge);
            self.target.DrawGeometry(&shape, &self.brush, 1.0, None::<&ID2D1StrokeStyle>);
        }
    }

    pub fn fill_squircle(&self, x: f32, y: f32, w: f32, h: f32, radius: f32, color: Color) {
        let Some(shape) = self.squircle(x, y, w, h, radius) else { return };
        self.set(color);
        unsafe { self.target.FillGeometry(&shape, &self.brush, None::<&ID2D1Brush>) }
    }

    pub fn stroke_squircle(&self, x: f32, y: f32, w: f32, h: f32, radius: f32, width: f32, color: Color) {
        let Some(shape) = self.squircle(x, y, w, h, radius) else { return };
        self.set(color);
        unsafe { self.target.DrawGeometry(&shape, &self.brush, width, None::<&ID2D1StrokeStyle>) }
    }

    pub fn fill_round(&self, x: f32, y: f32, w: f32, h: f32, radius: f32, color: Color) {
        self.set(color);
        let shape = D2D1_ROUNDED_RECT { rect: rect(x, y, w, h), radiusX: radius, radiusY: radius };
        unsafe { self.target.FillRoundedRectangle(&shape, &self.brush) }
    }

    /// A crescent, opening to the right.
    pub fn fill_moon(&self, x: f32, y: f32, radius: f32, color: Color) {
        let top = D2D_POINT_2F { x: x + radius * 0.5, y: y - radius * 0.866 };
        let bottom = D2D_POINT_2F { x: x + radius * 0.5, y: y + radius * 0.866 };
        let arc = |point, size, large| D2D1_ARC_SEGMENT {
            point,
            size: D2D_SIZE_F { width: size, height: size },
            rotationAngle: 0.0,
            sweepDirection: D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE,
            arcSize: if large { D2D1_ARC_SIZE_LARGE } else { D2D1_ARC_SIZE_SMALL },
        };
        unsafe {
            let Ok(path) = self.factory.CreatePathGeometry() else { return };
            let Ok(sink) = path.Open() else { return };
            sink.BeginFigure(top, D2D1_FIGURE_BEGIN_FILLED);
            sink.AddArc(&arc(bottom, radius, true));
            sink.AddArc(&D2D1_ARC_SEGMENT { sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE, ..arc(top, radius * 1.05, false) });
            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
            if sink.Close().is_ok() {
                self.set(color);
                self.target.FillGeometry(&path, &self.brush, None::<&ID2D1Brush>);
            }
        }
    }

    pub fn fill_circle(&self, x: f32, y: f32, radius: f32, color: Color) {
        self.set(color);
        let shape = D2D1_ELLIPSE { point: D2D_POINT_2F { x, y }, radiusX: radius, radiusY: radius };
        unsafe { self.target.FillEllipse(&shape, &self.brush) }
    }

    pub fn stroke_circle(&self, x: f32, y: f32, radius: f32, width: f32, color: Color) {
        self.set(color);
        let shape = D2D1_ELLIPSE { point: D2D_POINT_2F { x, y }, radiusX: radius, radiusY: radius };
        unsafe { self.target.DrawEllipse(&shape, &self.brush, width, None::<&ID2D1StrokeStyle>) }
    }

    pub fn line(&self, x0: f32, y0: f32, x1: f32, y1: f32, width: f32, color: Color) {
        self.set(color);
        unsafe {
            self.target.DrawLine(
                D2D_POINT_2F { x: x0, y: y0 },
                D2D_POINT_2F { x: x1, y: y1 },
                &self.brush,
                width,
                None::<&ID2D1StrokeStyle>,
            )
        }
    }

    pub fn text(&self, text: &str, size: f32, weight: Weight, align: Align, frame: Frame, color: Color) {
        self.draw_text(text, size, weight, false, align, frame, color);
    }

    pub fn number(&self, text: &str, size: f32, weight: Weight, align: Align, frame: Frame, color: Color) {
        self.draw_text(text, size, weight, true, align, frame, color);
    }

    fn draw_text(&self, text: &str, size: f32, weight: Weight, number: bool, align: Align, frame: Frame, color: Color) {
        self.set(color);
        let format = self.font(size, weight, number, align);
        let wide: Vec<u16> = text.encode_utf16().collect();
        let (x, y, w, h) = frame;
        unsafe {
            self.target.DrawText(
                &wide,
                &format,
                &rect(x, y, w, h),
                &self.brush,
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                DWRITE_MEASURING_MODE_NATURAL,
            )
        }
    }

    /// Runs `draw` clipped to a rectangle and squashed vertically about `hinge`: the two halves of a flip tile.
    pub fn folded(&self, clip: Frame, hinge: f32, scale: f32, draw: impl FnOnce()) {
        let transform = Matrix3x2 { M11: 1.0, M12: 0.0, M21: 0.0, M22: scale, M31: 0.0, M32: hinge * (1.0 - scale) };
        unsafe {
            self.target.SetTransform(&transform);
            self.target.PushAxisAlignedClip(&rect(clip.0, clip.1, clip.2, clip.3), D2D1_ANTIALIAS_MODE_ALIASED);
            draw();
            self.target.PopAxisAlignedClip();
            self.target.SetTransform(&Matrix3x2::identity());
        }
    }
}

/// x, y, width, height.
pub type Frame = (f32, f32, f32, f32);

fn rect(x: f32, y: f32, w: f32, h: f32) -> D2D_RECT_F {
    D2D_RECT_F { left: x, top: y, right: x + w, bottom: y + h }
}
