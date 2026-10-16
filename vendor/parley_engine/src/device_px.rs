// Copyright 2026 the Parley Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! wkhtmltopdf-rs: QtWebKit 2.2 measures and places text in whole (and 26.6 fixed point)
//! *device* pixels: under `--zoom` or `--dpi`, a CSS px is `zoom × dpi / 96` of them, and the
//! fonts are measured at that size. Layout stays in CSS px; these helpers round a CSS length in
//! device pixels and return it in CSS px.

use core::sync::atomic::{AtomicU32, Ordering};

static DEVICE_ZOOM: AtomicU32 = AtomicU32::new(0x3f80_0000);

/// Set the device pixels per CSS px of the next layouts (1 by default).
pub fn set_device_zoom(zoom: f32) {
    let zoom = if zoom.is_finite() && zoom > 0.0 { zoom } else { 1.0 };
    DEVICE_ZOOM.store(zoom.to_bits(), Ordering::Relaxed);
}

/// Device pixels per CSS px.
#[inline]
pub fn device_zoom() -> f32 {
    f32::from_bits(DEVICE_ZOOM.load(Ordering::Relaxed))
}

#[inline]
fn in_device(x: f32, f: impl Fn(f32) -> f32) -> f32 {
    let z = device_zoom();
    if z == 1.0 { f(x) } else { f(x * z) / z }
}

/// `x` rounded to whole device pixels.
#[inline]
pub fn round(x: f32) -> f32 {
    in_device(x, f32::round)
}

/// `x` rounded up to whole device pixels.
#[inline]
pub fn ceil(x: f32) -> f32 {
    in_device(x, f32::ceil)
}

/// `x` rounded down to whole device pixels.
#[inline]
pub fn floor(x: f32) -> f32 {
    in_device(x, f32::floor)
}

/// `x` truncated to whole device pixels.
#[inline]
pub fn trunc(x: f32) -> f32 {
    in_device(x, f32::trunc)
}

/// `x` truncated to 26.6 fixed point in device pixels.
#[inline]
pub fn floor_26_6(x: f32) -> f32 {
    in_device(x, |d| (d * 64.).floor() / 64.)
}
