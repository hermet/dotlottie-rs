#[allow(unused_imports)]
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Mutex;

#[allow(unused_imports)]
use instant::Instant;

#[cfg(target_arch = "wasm32")]
use spin::Mutex;

use std::{ffi::CString, ptr, result::Result};
use thiserror::Error;

use super::{Animation, ColorSpace, Drawable, Renderer, Shape};

#[expect(non_upper_case_globals)]
#[allow(non_snake_case)]
#[expect(non_camel_case_types)]
#[expect(dead_code)]
mod tvg {
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}

#[derive(Error, Debug)]
pub enum TvgError {
    #[error("Invalid argument")]
    InvalidArgument,
    #[error("Insufficient condition")]
    InsufficientCondition,
    #[error("Failed allocation")]
    FailedAllocation,
    #[error("Memory corruption")]
    MemoryCorruption,
    #[error("Not supported")]
    NotSupported,
    #[error("Unknown error")]
    Unknown,
}

pub trait IntoResult {
    fn into_result(self) -> Result<(), TvgError>;
}

impl IntoResult for tvg::Tvg_Result {
    fn into_result(self) -> Result<(), TvgError> {
        match self {
            tvg::Tvg_Result_TVG_RESULT_SUCCESS => Ok(()),
            tvg::Tvg_Result_TVG_RESULT_INVALID_ARGUMENT => Err(TvgError::InvalidArgument),
            tvg::Tvg_Result_TVG_RESULT_INSUFFICIENT_CONDITION => {
                Err(TvgError::InsufficientCondition)
            }
            tvg::Tvg_Result_TVG_RESULT_FAILED_ALLOCATION => Err(TvgError::FailedAllocation),
            tvg::Tvg_Result_TVG_RESULT_MEMORY_CORRUPTION => Err(TvgError::MemoryCorruption),
            tvg::Tvg_Result_TVG_RESULT_NOT_SUPPORTED => Err(TvgError::NotSupported),
            tvg::Tvg_Result_TVG_RESULT_UNKNOWN => Err(TvgError::Unknown),
            _ => unreachable!(),
        }
    }
}

impl From<ColorSpace> for tvg::Tvg_Colorspace {
    fn from(color_space: ColorSpace) -> Self {
        match color_space {
            ColorSpace::ABGR8888 => tvg::Tvg_Colorspace_TVG_COLORSPACE_ABGR8888,
            ColorSpace::ABGR8888S => tvg::Tvg_Colorspace_TVG_COLORSPACE_ABGR8888S,
            ColorSpace::ARGB8888 => tvg::Tvg_Colorspace_TVG_COLORSPACE_ARGB8888,
            ColorSpace::ARGB8888S => tvg::Tvg_Colorspace_TVG_COLORSPACE_ARGB8888S,
        }
    }
}

pub enum TvgEngine {
    TvgEngineSw,
    TvgEngineGl,
}

impl From<TvgEngine> for tvg::Tvg_Engine {
    fn from(engine_method: TvgEngine) -> Self {
        match engine_method {
            TvgEngine::TvgEngineSw => tvg::Tvg_Engine_TVG_ENGINE_SW,
            TvgEngine::TvgEngineGl => tvg::Tvg_Engine_TVG_ENGINE_GL,
        }
    }
}

static RENDERERS_COUNT: spin::Mutex<usize> = spin::Mutex::new(0);

pub struct TvgRenderer {
    raw_canvas: *mut tvg::Tvg_Canvas,
    engine_method: tvg::Tvg_Engine,
}

impl TvgRenderer {
    pub fn new(engine_method: TvgEngine, threads: u32) -> Self {
        let engine = engine_method.into();

        let mut count = RENDERERS_COUNT.lock();

        if *count == 0 {
            unsafe { tvg::tvg_engine_init(engine, threads).into_result() }
                .expect("Failed to initialize ThorVG engine");
        }

        *count += 1;

        TvgRenderer {
            raw_canvas: unsafe { tvg::tvg_swcanvas_create() },
            engine_method: engine,
        }
    }
}

impl Renderer for TvgRenderer {
    type Animation = TvgAnimation;
    type Shape = TvgShape;
    type Error = TvgError;

    fn set_viewport(&mut self, x: i32, y: i32, w: i32, h: i32) -> Result<(), TvgError> {
        unsafe { tvg::tvg_canvas_set_viewport(self.raw_canvas, x, y, w, h).into_result() }
    }

    fn set_target(
        &mut self,
        buffer: &mut Vec<u32>,
        stride: u32,
        width: u32,
        height: u32,
        color_space: ColorSpace,
    ) -> Result<(), TvgError> {
        unsafe {
            tvg::tvg_swcanvas_set_target(
                self.raw_canvas,
                buffer.as_mut_ptr(),
                stride,
                width,
                height,
                color_space.into(),
            )
            .into_result()
        }
    }

    fn clear(&self, _free: bool) -> Result<(), TvgError> {
        #[cfg(feature = "thorvg-v1")]
        unsafe {
            tvg::tvg_canvas_remove(self.raw_canvas, ptr::null_mut::<tvg::Tvg_Paint>()).into_result()
        }

        #[cfg(feature = "thorvg-v0")]
        unsafe {
            tvg::tvg_canvas_clear(self.raw_canvas, _free).into_result()
        }
    }

    fn push(&mut self, drawable: Drawable<Self>) -> Result<(), TvgError> {
        let raw_paint = match drawable {
            Drawable::Animation(animation) => animation.raw_paint,
            Drawable::Shape(shape) => shape.raw_shape,
        };

        unsafe { tvg::tvg_canvas_push(self.raw_canvas, raw_paint).into_result() }
    }

    fn draw(&mut self, _clear_buffer: bool) -> Result<(), TvgError> {
        #[cfg(feature = "thorvg-v1")]
        unsafe {
            tvg::tvg_canvas_draw(self.raw_canvas, _clear_buffer).into_result()
        }

        #[cfg(feature = "thorvg-v0")]
        unsafe {
            tvg::tvg_canvas_draw(self.raw_canvas).into_result()
        }
    }

    fn sync(&mut self) -> Result<(), TvgError> {
        unsafe { tvg::tvg_canvas_sync(self.raw_canvas).into_result() }
    }

    fn update(&mut self) -> Result<(), TvgError> {
        unsafe { tvg::tvg_canvas_update(self.raw_canvas).into_result() }
    }
}

impl Drop for TvgRenderer {
    fn drop(&mut self) {
        let mut count = RENDERERS_COUNT.lock();

        unsafe {
            tvg::tvg_canvas_destroy(self.raw_canvas);
        }

        *count = count.checked_sub(1).unwrap();

        if *count == 0 {
            unsafe { tvg::tvg_engine_term(self.engine_method) };
        }
    }
}

#[cfg(feature = "thorvg-v1")]
struct TweenState {
    start_time: Instant,
    from: f32,
    to: f32,
    duration: f32,
    easing: [f32; 4],
}

pub struct TvgAnimation {
    raw_animation: *mut tvg::Tvg_Animation,
    raw_paint: *mut tvg::Tvg_Paint,
    #[cfg(feature = "thorvg-v1")]
    tween_state: Option<TweenState>,
}

impl Default for TvgAnimation {
    fn default() -> Self {
        let raw_animation = unsafe { tvg::tvg_animation_new() };
        let raw_paint = unsafe { tvg::tvg_animation_get_picture(raw_animation) };

        Self {
            raw_animation,
            raw_paint,
            #[cfg(feature = "thorvg-v1")]
            tween_state: None,
        }
    }
}

impl Animation for TvgAnimation {
    type Error = TvgError;

    fn load_data(&mut self, data: &str, mimetype: &str, copy: bool) -> Result<(), TvgError> {
        let mimetype_cstr = CString::new(mimetype).unwrap();
        let data_cstr = CString::new(data).unwrap();
        let data_len = data_cstr.as_bytes().len() as u32;

        #[cfg(feature = "thorvg-v1")]
        unsafe {
            let data_ptr = data_cstr.as_ptr();
            let mimetype_ptr = mimetype_cstr.as_ptr();
            tvg::tvg_picture_load_data(
                self.raw_paint,
                data_ptr,
                data_len,
                mimetype_ptr,
                ptr::null(),
                copy,
            )
            .into_result()
        }

        #[cfg(feature = "thorvg-v0")]
        unsafe {
            let data_ptr = data_cstr.as_ptr();
            let mimetype_ptr = mimetype_cstr.as_ptr();
            tvg::tvg_picture_load_data(self.raw_paint, data_ptr, data_len, mimetype_ptr, copy)
                .into_result()
        }
    }

    fn get_layer_bounds(&self, layer_name: &str) -> Result<(f32, f32, f32, f32), TvgError> {
        let paint = self.raw_paint;
        let layer_name_cstr = CString::new(layer_name).expect("Failed to create CString");
        let layer_id = unsafe { tvg::tvg_accessor_generate_id(layer_name_cstr.as_ptr()) };
        let layer = unsafe { tvg::tvg_picture_get_paint(paint, layer_id) };

        if !layer.is_null() {
            let mut px: f32 = -1.0;
            let mut py: f32 = -1.0;
            let mut pw: f32 = -1.0;
            let mut ph: f32 = -1.0;

            let bounds = unsafe {
                tvg::tvg_paint_get_bounds(
                    layer,
                    &mut px as *mut f32,
                    &mut py as *mut f32,
                    &mut pw as *mut f32,
                    &mut ph as *mut f32,
                    true,
                )
            };

            bounds.into_result()?;

            Ok((px, py, pw, ph))
        } else {
            Err(TvgError::Unknown)
        }
    }

    fn hit_check(&self, layer_name: &str, x: f32, y: f32) -> Result<bool, TvgError> {
        let paint = self.raw_paint;
        let layer_name_cstr = CString::new(layer_name).expect("Failed to create CString");
        let layer_id = unsafe { tvg::tvg_accessor_generate_id(layer_name_cstr.as_ptr()) };
        let layer = unsafe { tvg::tvg_picture_get_paint(paint, layer_id) };

        if !layer.is_null() {
            let mut px: f32 = -1.0;
            let mut py: f32 = -1.0;
            let mut pw: f32 = -1.0;
            let mut ph: f32 = -1.0;

            let bounds = unsafe {
                tvg::tvg_paint_get_bounds(
                    layer,
                    &mut px as *mut f32,
                    &mut py as *mut f32,
                    &mut pw as *mut f32,
                    &mut ph as *mut f32,
                    true,
                )
            };

            bounds.into_result()?;

            if x >= px && x <= px + pw && y >= py && y <= py + ph {
                return Ok(true);
            }
        }

        Ok(false)
    }

    fn get_size(&self) -> Result<(f32, f32), TvgError> {
        let mut width = 0.0;
        let mut height = 0.0;

        unsafe {
            tvg::tvg_picture_get_size(
                self.raw_paint,
                &mut width as *mut f32,
                &mut height as *mut f32,
            )
            .into_result()
        }?;

        Ok((width, height))
    }

    fn set_size(&mut self, width: f32, height: f32) -> Result<(), TvgError> {
        unsafe { tvg::tvg_picture_set_size(self.raw_paint, width, height).into_result() }
    }

    fn scale(&mut self, factor: f32) -> Result<(), TvgError> {
        unsafe { tvg::tvg_paint_scale(self.raw_paint, factor).into_result() }
    }

    fn translate(&mut self, tx: f32, ty: f32) -> Result<(), TvgError> {
        unsafe { tvg::tvg_paint_translate(self.raw_paint, tx, ty).into_result() }
    }

    fn get_total_frame(&self) -> Result<f32, TvgError> {
        let mut total_frame: f32 = 0.0;

        unsafe {
            tvg::tvg_animation_get_total_frame(self.raw_animation, &mut total_frame as *mut f32)
                .into_result()
        }?;

        Ok(total_frame)
    }

    fn get_duration(&self) -> Result<f32, TvgError> {
        let mut duration: f32 = 0.0;

        unsafe {
            tvg::tvg_animation_get_duration(self.raw_animation, &mut duration as *mut f32)
                .into_result()
        }?;

        Ok(duration)
    }

    fn set_frame(&mut self, frame_no: f32) -> Result<(), TvgError> {
        unsafe { tvg::tvg_animation_set_frame(self.raw_animation, frame_no).into_result() }
    }

    fn get_frame(&self) -> Result<f32, TvgError> {
        let mut curr_frame: f32 = 0.0;

        unsafe {
            tvg::tvg_animation_get_frame(self.raw_animation, &mut curr_frame as *mut f32)
                .into_result()
        }?;

        Ok(curr_frame)
    }

    fn set_slots(&mut self, slots: &str) -> Result<(), TvgError> {
        let result = if slots.is_empty() {
            unsafe { tvg::tvg_lottie_animation_override(self.raw_animation, ptr::null()) }
        } else {
            let slots_cstr = CString::new(slots).expect("Failed to create CString");
            unsafe { tvg::tvg_lottie_animation_override(self.raw_animation, slots_cstr.as_ptr()) }
        };

        result.into_result()
    }

    fn tween(&mut self, _from: f32, _to: f32, _progress: f32) -> Result<(), TvgError> {
        #[cfg(feature = "thorvg-v1")]
        {
            if self.is_tweening() || _progress <= 0.0 {
                return Err(TvgError::InvalidArgument);
            }

            unsafe {
                tvg::tvg_lottie_animation_tween(self.raw_animation, _from, _to, _progress)
                    .into_result()
            }
        }

        #[cfg(not(feature = "thorvg-v1"))]
        Err(TvgError::NotSupported)
    }

    fn tween_to(&mut self, _to: f32, _duration: f32, _easing: [f32; 4]) -> Result<(), TvgError> {
        #[cfg(feature = "thorvg-v1")]
        {
            if self.is_tweening() || _duration <= 0.0 {
                return Err(TvgError::InvalidArgument);
            }

            self.tween_state = Some(TweenState {
                start_time: Instant::now(),
                from: self.get_frame()?,
                to: _to,
                duration: _duration,
                easing: _easing,
            });

            Ok(())
        }

        #[cfg(not(feature = "thorvg-v1"))]
        Err(TvgError::NotSupported)
    }

    fn is_tweening(&self) -> bool {
        #[cfg(feature = "thorvg-v1")]
        return self.tween_state.is_some();

        #[cfg(not(feature = "thorvg-v1"))]
        false
    }

    fn tween_update(&mut self) -> Result<bool, TvgError> {
        #[cfg(feature = "thorvg-v1")]
        {
            if let Some(tween_state) = self.tween_state.as_mut() {
                let elapsed = Instant::now().duration_since(tween_state.start_time);
                let t = elapsed.as_secs_f32() / tween_state.duration;
                let progress = if t >= 1.0 {
                    1.0
                } else {
                    let [x1, y1, x2, y2] = tween_state.easing;
                    bezier::cubic_bezier(t, x1, y1, x2, y2)
                };

                unsafe {
                    tvg::tvg_lottie_animation_tween(
                        self.raw_animation,
                        tween_state.from,
                        tween_state.to,
                        progress,
                    );
                };

                if progress >= 1.0 {
                    let target_frame = tween_state.to;
                    self.tween_state = None;
                    self.set_frame(target_frame)?;
                    Ok(false)
                } else {
                    Ok(true)
                }
            } else {
                Ok(false)
            }
        }

        #[cfg(not(feature = "thorvg-v1"))]
        Err(TvgError::NotSupported)
    }
}

impl Drop for TvgAnimation {
    fn drop(&mut self) {
        unsafe {
            tvg::tvg_animation_del(self.raw_animation);
        };
    }
}

pub struct TvgShape {
    raw_shape: *mut tvg::Tvg_Paint,
}

impl Default for TvgShape {
    fn default() -> Self {
        Self {
            raw_shape: unsafe { tvg::tvg_shape_new() },
        }
    }
}

impl Shape for TvgShape {
    type Error = TvgError;

    fn fill(&mut self, color: (u8, u8, u8, u8)) -> Result<(), TvgError> {
        unsafe {
            tvg::tvg_shape_set_fill_color(self.raw_shape, color.0, color.1, color.2, color.3)
                .into_result()
        }
    }

    fn append_rect(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        rx: f32,
        ry: f32,
    ) -> Result<(), TvgError> {
        #[cfg(feature = "thorvg-v1")]
        unsafe {
            tvg::tvg_shape_append_rect(self.raw_shape, x, y, w, h, rx, ry, true).into_result()
        }

        #[cfg(feature = "thorvg-v0")]
        unsafe {
            tvg::tvg_shape_append_rect(self.raw_shape, x, y, w, h, rx, ry).into_result()
        }
    }

    fn reset(&mut self) -> Result<(), TvgError> {
        unsafe { tvg::tvg_shape_reset(self.raw_shape).into_result() }
    }
}

#[cfg(feature = "thorvg-v1")]
mod bezier {
    /// Computes the x-coordinate of the cubic Bézier for parameter `u`.
    /// P0 = 0, P1 = (x1, _), P2 = (x2, _), P3 = 1.
    pub(super) fn sample_curve_x(u: f32, x1: f32, x2: f32) -> f32 {
        let inv_u = 1.0 - u;
        3.0 * inv_u * inv_u * u * x1 + 3.0 * inv_u * u * u * x2 + u * u * u
    }

    /// Computes the y-coordinate of the cubic Bézier for parameter `u`.
    /// P0 = 0, P1 = (_, y1), P2 = (_, y2), P3 = 1.
    pub(super) fn sample_curve_y(u: f32, y1: f32, y2: f32) -> f32 {
        let inv_u = 1.0 - u;
        3.0 * inv_u * inv_u * u * y1 + 3.0 * inv_u * u * u * y2 + u * u * u
    }

    /// Computes the derivative dx/du for a given u.
    fn sample_curve_derivative_x(u: f32, x1: f32, x2: f32) -> f32 {
        let inv_u = 1.0 - u;
        3.0 * inv_u * inv_u * x1 + 6.0 * inv_u * u * (x2 - x1) + 3.0 * u * u * (1.0 - x2)
    }

    /// Uses binary subdivision to find a parameter u such that sample_curve_x(u) ≈ t.
    fn binary_subdivide(t: f32, x1: f32, x2: f32) -> f32 {
        let mut a = 0.0;
        let mut b = 1.0;
        let mut u = t;
        for _ in 0..10 {
            let x = sample_curve_x(u, x1, x2);
            if (x - t).abs() < 1e-6 {
                return u;
            }
            if x > t {
                b = u;
            } else {
                a = u;
            }
            u = (a + b) * 0.5;
        }
        u
    }

    /// Given a linear progress t in [0,1], uses a cubic Bézier easing function to compute
    /// an eased progress value in [0,1].  
    ///  
    /// The cubic Bézier is defined by:
    ///   P0 = (0, 0)
    ///   P1 = (x1, y1)
    ///   P2 = (x2, y2)
    ///   P3 = (1, 1)
    pub(super) fn cubic_bezier(t: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
        if t <= 0.0 {
            return 0.0;
        }
        if t >= 1.0 {
            return 1.0;
        }

        // First try Newton–Raphson iteration.
        let mut u = t;
        for _ in 0..8 {
            let x = sample_curve_x(u, x1, x2);
            let dx = sample_curve_derivative_x(u, x1, x2);
            if dx.abs() < 1e-6 {
                break;
            }
            let delta = (x - t) / dx;
            u -= delta;
            if delta.abs() < 1e-6 {
                break;
            }
        }

        // Fallback to binary subdivision if necessary.
        if !(0.0..=1.0).contains(&u) {
            u = binary_subdivide(t, x1, x2);
        }
        u = u.clamp(0.0, 1.0);
        sample_curve_y(u, y1, y2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    use std::thread;

    #[test]
    fn test_tvg_renderer_no_deadlock() {
        const THREAD_COUNT: usize = 10;
        let barrier = Arc::new(Barrier::new(THREAD_COUNT));
        let mut handles = vec![];

        for _ in 0..THREAD_COUNT {
            let barrier_clone = Arc::clone(&barrier);
            let handle = thread::spawn(move || {
                barrier_clone.wait();

                let renderer = TvgRenderer::new(TvgEngine::TvgEngineSw, 0);
                drop(renderer);
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().expect("Thread panicked");
        }
    }
}
