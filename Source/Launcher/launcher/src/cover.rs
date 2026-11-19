use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;

use eframe::egui::{self, Color32, ColorImage, TextureHandle, TextureOptions};
use openh264::decoder::{DecodedYUV, Decoder};
use openh264::formats::YUVSource;
use openh264::nal_units;

const VIDEO: &[u8] = include_bytes!("../../assets/ui/cover.h264");
const FRAME_SECONDS: f64 = 1001.0 / 30000.0;

pub struct Cover {
    texture: TextureHandle,
    frames: Option<Receiver<ColorImage>>,
    next_at: f64,
    ctx: egui::Context,
}

struct Tables {
    y: [i32; 256],
    rv: [i32; 256],
    gu: [i32; 256],
    gv: [i32; 256],
    bu: [i32; 256],
}

fn tables() -> &'static Tables {
    static TABLES: OnceLock<Tables> = OnceLock::new();
    TABLES.get_or_init(|| {
        let mut t = Tables { y: [0; 256], rv: [0; 256], gu: [0; 256], gv: [0; 256], bu: [0; 256] };
        for i in 0..256 {
            let c = i as i32;
            t.y[i] = 298 * (c - 16) + 128;
            t.rv[i] = 409 * (c - 128);
            t.gu[i] = -100 * (c - 128);
            t.gv[i] = -208 * (c - 128);
            t.bu[i] = 516 * (c - 128);
        }
        t
    })
}

fn channel(value: i32) -> u8 {
    (value >> 8).clamp(0, 255) as u8
}

fn picture(yuv: &DecodedYUV<'_>) -> ColorImage {
    let (width, height) = yuv.dimensions();
    let (y_stride, u_stride, v_stride) = yuv.strides();
    let (luma, blue, red) = (yuv.y(), yuv.u(), yuv.v());
    let t = tables();
    let mut pixels = Vec::with_capacity(width * height);
    for row in 0..height {
        let y_row = &luma[row * y_stride..row * y_stride + width];
        let u_row = &blue[(row / 2) * u_stride..(row / 2) * u_stride + width.div_ceil(2)];
        let v_row = &red[(row / 2) * v_stride..(row / 2) * v_stride + width.div_ceil(2)];
        for ((ys, &u), &v) in y_row.chunks(2).zip(u_row).zip(v_row) {
            let (r, g, b) = (t.rv[v as usize], t.gu[u as usize] + t.gv[v as usize], t.bu[u as usize]);
            for &y in ys {
                let l = t.y[y as usize];
                pixels.push(Color32::from_rgb(channel(l + r), channel(l + g), channel(l + b)));
            }
        }
    }
    ColorImage { size: [width, height], pixels }
}

fn first_frame() -> Option<ColorImage> {
    let mut decoder = Decoder::new().ok()?;
    for nal in nal_units(VIDEO) {
        if let Ok(Some(yuv)) = decoder.decode(nal) {
            return Some(picture(&yuv));
        }
    }
    None
}

fn play(frames: SyncSender<ColorImage>) {
    loop {
        let Ok(mut decoder) = Decoder::new() else { return };
        let mut shown = 0;
        for nal in nal_units(VIDEO) {
            if let Ok(Some(yuv)) = decoder.decode(nal) {
                if frames.send(picture(&yuv)).is_err() {
                    return;
                }
                shown += 1;
            }
        }
        if let Ok(rest) = decoder.flush_remaining() {
            for yuv in rest {
                if frames.send(picture(&yuv)).is_err() {
                    return;
                }
                shown += 1;
            }
        }
        if shown == 0 {
            return;
        }
    }
}

impl Cover {
    pub fn new(ctx: &egui::Context) -> Cover {
        let first = first_frame().unwrap_or_else(|| ColorImage::new([2, 1], Color32::from_rgb(40, 12, 70)));
        Cover { texture: ctx.load_texture("cover-video", first, TextureOptions::LINEAR), frames: None, next_at: 0.0, ctx: ctx.clone() }
    }

    pub fn texture(&self) -> &TextureHandle {
        &self.texture
    }

    pub fn tick(&mut self, playing: bool, now: f64) {
        if !playing {
            return;
        }
        let frames = self.frames.get_or_insert_with(|| {
            let (tx, rx) = sync_channel(2);
            let _ = thread::Builder::new().name("cover-video".into()).spawn(move || play(tx));
            rx
        });
        if now >= self.next_at {
            if let Ok(image) = frames.try_recv() {
                self.texture.set(image, TextureOptions::LINEAR);
                self.next_at = if now - self.next_at > 0.25 { now + FRAME_SECONDS } else { self.next_at + FRAME_SECONDS };
            }
        }
        self.ctx.request_repaint_after(Duration::from_secs_f64((self.next_at - now).clamp(0.004, FRAME_SECONDS)));
    }
}
