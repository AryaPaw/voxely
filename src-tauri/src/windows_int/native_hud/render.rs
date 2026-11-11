use super::*;

#[derive(Clone, Copy)]
pub(super) struct Color(pub [u8; 3]);

#[cfg(test)]
pub(super) fn token(name: &str, dark: bool) -> Option<Color> {
    let [r, g, b, _] = crate::theme_tokens::color(&format!("overlay-{name}"), dark).ok()?;
    Some(Color([r, g, b].map(|value| (value * 255.0).round() as u8)))
}

struct Palette {
    bg: Color,
    fg: Color,
    border: Color,
    armed: Color,
    cancel: Color,
    status: Color,
    hover_bg: Color,
}
impl Palette {
    fn load(dark: bool, accent: &crate::settings::AccentColor) -> Option<Self> {
        let token = |name: &str, dark| {
            let [r, g, b, _] =
                crate::theme_tokens::color_with_accent(&format!("overlay-{name}"), dark, accent)
                    .ok()?;
            Some(Color([r, g, b].map(|v| (v * 255.0).round() as u8)))
        };
        Some(Self {
            bg: token("bg", dark)?,
            fg: token("fg", dark)?,
            border: token("border", dark)?,
            armed: token("armed", dark)?,
            cancel: token("armed-fg", dark)?,
            status: token("status", dark)?,
            hover_bg: token("hover-bg", dark)?,
        })
    }
}

pub(super) fn copy(key: &str, locale: &str) -> Option<&'static str> {
    crate::app::locale::raw(key, locale)
}

pub(super) fn label(frame: &Frame, hovered: bool) -> String {
    let key = if hovered {
        "overlayCancel"
    } else if frame.snapshot.limit_reached
        && matches!(
            frame.snapshot.state,
            SessionState::StartingRecording
                | SessionState::Recording
                | SessionState::StoppingRecording
                | SessionState::Saving
        )
    {
        "overlayRecordingLimit"
    } else {
        match frame.snapshot.state {
            SessionState::StartingRecording | SessionState::Recording => "overlayRecording",
            SessionState::StoppingRecording | SessionState::Saving => "overlaySaving",
            SessionState::ProcessingAudio => "overlayProcessing",
            SessionState::Transcribing { .. } | SessionState::RetryWaiting { .. } => {
                "overlayTranscribing"
            }
            SessionState::Failed { .. } => "overlayError",
            _ => "overlayReady",
        }
    };
    let mut text = copy(key, &frame.locale).unwrap_or("Voxely").to_string();
    let attempt = match frame.snapshot.state {
        SessionState::Transcribing { attempt } => attempt,
        SessionState::RetryWaiting { attempt, .. } => attempt.saturating_add(1),
        _ => 0,
    };
    if !hovered && attempt > 1 {
        text = copy("overlayRetry", &frame.locale)
            .unwrap_or("Voxely")
            .replace("{attempt}", &attempt.to_string());
    }
    text
}

// Same 296 x 52 logical pill and 320 x 72 transparent canvas as the existing HUD.
fn distance(x: f64, y: f64, inset: f64) -> f64 {
    let radius = 26.0 - inset;
    let dx = (x - 160.0).abs() - (148.0 - inset - radius);
    let dy = (y - 34.0).abs() - (26.0 - inset - radius);
    dx.max(0.0).hypot(dy.max(0.0)) + dx.max(dy).min(0.0) - radius
}
pub(super) fn pill_contains(x: f64, y: f64) -> bool {
    distance(x, y, 0.0) <= 0.0
}

struct Dib {
    dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    bits: *mut u8,
    width: i32,
    height: i32,
}
impl Dib {
    unsafe fn new(width: i32, height: i32) -> Result<Self> {
        let dc = CreateCompatibleDC(None);
        if dc.0.is_null() {
            return Err(windows::core::Error::from_win32());
        }
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let bitmap = match CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, None, 0) {
            Ok(bitmap) => bitmap,
            Err(error) => {
                let _ = DeleteDC(dc);
                return Err(error);
            }
        };
        let previous = SelectObject(dc, bitmap);
        Ok(Self {
            dc,
            bitmap,
            previous,
            bits: bits.cast(),
            width,
            height,
        })
    }
    unsafe fn pixels(&mut self) -> &mut [u8] {
        std::slice::from_raw_parts_mut(self.bits, (self.width * self.height * 4) as usize)
    }
}
impl Drop for Dib {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.previous);
            let _ = DeleteObject(self.bitmap);
            let _ = DeleteDC(self.dc);
        }
    }
}

pub(super) struct Painter {
    canvas: Dib,
    mask: Dib,
    font: HFONT,
    old_font: HGDIOBJ,
    scale: f64,
    palette: Option<(bool, crate::settings::AccentColor, Palette)>,
}
impl Painter {
    pub(super) unsafe fn new(scale: f64) -> Result<Self> {
        let (width, height) = (
            (320.0 * scale).round() as i32,
            (72.0 * scale).round() as i32,
        );
        let canvas = Dib::new(width, height)?;
        let mask = Dib::new(width, height)?;
        // Grayscale coverage avoids ClearType fringes on a translucent surface.
        let font = CreateFontW(
            -(13.0 * scale).round() as i32,
            0,
            0,
            0,
            500,
            0,
            0,
            0,
            DEFAULT_CHARSET.0 as u32,
            OUT_DEFAULT_PRECIS.0 as u32,
            CLIP_DEFAULT_PRECIS.0 as u32,
            ANTIALIASED_QUALITY.0 as u32,
            0,
            w!("Segoe UI"),
        );
        if font.0.is_null() {
            return Err(windows::core::Error::from_win32());
        }
        let old_font = SelectObject(mask.dc, font);
        Ok(Self {
            canvas,
            mask,
            font,
            old_font,
            scale,
            palette: None,
        })
    }

    pub(super) fn scale(&self) -> f64 {
        self.scale
    }

    pub(super) unsafe fn paint(
        &mut self,
        hwnd: HWND,
        frame: &Frame,
        hovered: bool,
        elapsed: Duration,
        position: POINT,
    ) -> Result<()> {
        let dark = match frame.theme.as_str() {
            "light" => false,
            "system" => super::system_dark(),
            _ => true,
        };
        if self
            .palette
            .as_ref()
            .is_none_or(|(cached_dark, accent, _)| {
                *cached_dark != dark || accent != &frame.accent_color
            })
        {
            let palette = Palette::load(dark, &frame.accent_color).ok_or_else(|| {
                windows::core::Error::new(
                    windows::core::HRESULT(0x80004005u32 as i32),
                    "Invalid overlay color tokens",
                )
            })?;
            self.palette = Some((dark, frame.accent_color.clone(), palette));
        }
        let p = &self.palette.as_ref().expect("palette initialized").2;
        let recording = matches!(
            frame.snapshot.state,
            SessionState::StartingRecording | SessionState::Recording
        );
        let scale = self.scale;
        let width = self.canvas.width;
        let pixels = self.canvas.pixels();
        for (i, pixel) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let x = (i as i32 % width) as f64 / scale;
            let y = (i as i32 / width) as f64 / scale;
            let alpha = ((0.5 - distance(x, y, 0.0) * scale).clamp(0.0, 1.0) * 255.0) as u8;
            let border = distance(x, y, 2.0) > 0.0;
            let color = if border {
                if hovered {
                    p.armed
                } else {
                    p.border
                }
            } else {
                if hovered {
                    p.hover_bg
                } else {
                    p.bg
                }
            };
            pixel.copy_from_slice(&[
                ((color.0[2] as u16 * alpha as u16) / 255) as u8,
                ((color.0[1] as u16 * alpha as u16) / 255) as u8,
                ((color.0[0] as u16 * alpha as u16) / 255) as u8,
                alpha,
            ]);
        }
        self.mask.pixels().fill(0);
        SetBkMode(self.mask.dc, TRANSPARENT);
        SetTextColor(self.mask.dc, COLORREF(0x00ffffff));
        let text = label(frame, hovered);
        let mut rect = RECT {
            left: (if hovered {
                30.0
            } else if recording {
                144.0
            } else {
                50.0
            } * scale) as i32,
            top: (8.0 * scale) as i32,
            right: (290.0 * scale) as i32,
            bottom: (60.0 * scale) as i32,
        };
        let mut text: Vec<u16> = text.encode_utf16().collect();
        DrawTextW(
            self.mask.dc,
            &mut text,
            &mut rect,
            DT_SINGLELINE
                | DT_VCENTER
                | DT_END_ELLIPSIS
                | if hovered { DT_CENTER } else { DT_LEFT },
        );
        if recording && !hovered && !frame.snapshot.limit_reached {
            let seconds = elapsed.as_secs();
            let mut clock: Vec<u16> = format!("{}:{:02}", seconds / 60, seconds % 60)
                .encode_utf16()
                .collect();
            rect.left = (251.0 * scale) as i32;
            rect.right = (289.0 * scale) as i32;
            DrawTextW(
                self.mask.dc,
                &mut clock,
                &mut rect,
                DT_SINGLELINE | DT_VCENTER | DT_RIGHT,
            );
        }
        // GDI queues drawing; flush before reading the DIB bytes.
        let _ = GdiFlush();
        let fg = if hovered { p.cancel } else { p.status };
        for (pixel, mask) in self
            .canvas
            .pixels()
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(self.mask.pixels().as_chunks::<4>().0.iter())
        {
            let a = u16::from(mask[0]);
            for (c, channel) in pixel.iter_mut().take(3).enumerate() {
                *channel =
                    ((u16::from(*channel) * (255 - a) + u16::from(fg.0[2 - c]) * a) / 255) as u8;
            }
        }
        if !hovered {
            let pixels = self.canvas.pixels();
            for (i, pixel) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let x = (i as i32 % width) as f64 / scale;
                let y = (i as i32 / width) as f64 / scale;
                let dot = (x - 35.0).hypot(y - 34.0) <= 3.5;
                let wave = recording && (52.0..136.0).contains(&x) && {
                    let index = ((x - 52.0) / 3.0) as usize;
                    let level = frame
                        .levels
                        .get(frame.levels.len().saturating_sub(28) + index)
                        .copied()
                        .unwrap_or(0.0);
                    (x - 52.0) % 3.0 < 1.7
                        && (y - 34.0).abs() <= 1.0 + 10.0 * f64::from(level.clamp(0.0, 1.0)).sqrt()
                };
                if dot || wave {
                    let c = if dot && recording { p.armed } else { p.fg };
                    pixel.copy_from_slice(&[c.0[2], c.0[1], c.0[0], 255]);
                }
            }
        }
        UpdateLayeredWindow(
            hwnd,
            None,
            Some(&position),
            Some(&SIZE {
                cx: self.canvas.width,
                cy: self.canvas.height,
            }),
            self.canvas.dc,
            Some(&POINT::default()),
            COLORREF(0),
            Some(&BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            }),
            ULW_ALPHA,
        )
    }
}
impl Drop for Painter {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.mask.dc, self.old_font);
            let _ = DeleteObject(self.font);
        }
    }
}
