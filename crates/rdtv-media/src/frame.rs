/// A decoded I420 picture owned by Rust (software decode path).
#[derive(Clone)]
pub struct YuvFrame {
    pub width: u32,
    pub height: u32,
    pub y: Vec<u8>,
    pub u: Vec<u8>,
    pub v: Vec<u8>,
    pub y_stride: usize,
    pub uv_stride: usize,
}

impl YuvFrame {
    /// Copy planes from decoder memory. Returns `None` for implausible geometry.
    ///
    /// # Safety
    /// `planes[i]` must be valid for `strides[i] * plane_height(i)` bytes.
    pub unsafe fn copy_i420(
        planes: [*const u8; 3],
        strides: [i32; 3],
        width: u32,
        height: u32,
    ) -> Option<YuvFrame> {
        if width == 0 || height == 0 || width > 8192 || height > 8192 {
            return None;
        }
        let (ys, us, vs) = (strides[0], strides[1], strides[2]);
        if ys < width as i32 || us < width.div_ceil(2) as i32 || vs < width.div_ceil(2) as i32 {
            return None;
        }
        let cw = width.div_ceil(2) as usize;
        let ch = height.div_ceil(2) as usize;
        let w = width as usize;
        let h = height as usize;
        let mut y = vec![0u8; w * h];
        let mut u = vec![0u8; cw * ch];
        let mut v = vec![0u8; cw * ch];
        for r in 0..h {
            let src = std::slice::from_raw_parts(planes[0].add(r * ys as usize), w);
            y[r * w..(r + 1) * w].copy_from_slice(src);
        }
        for r in 0..ch {
            let su = std::slice::from_raw_parts(planes[1].add(r * us as usize), cw);
            let sv = std::slice::from_raw_parts(planes[2].add(r * vs as usize), cw);
            u[r * cw..(r + 1) * cw].copy_from_slice(su);
            v[r * cw..(r + 1) * cw].copy_from_slice(sv);
        }
        Some(YuvFrame {
            width,
            height,
            y,
            u,
            v,
            y_stride: w,
            uv_stride: cw,
        })
    }
}
