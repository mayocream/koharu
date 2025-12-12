use candle_core::{CpuStorage, Layout, Result, Shape, bail};
use rustfft::{FftPlanner, num_complex::Complex32};

pub fn rfft2(storage: &CpuStorage, layout: &Layout) -> Result<(CpuStorage, Shape)> {
    let dims = layout.dims();
    if dims.len() != 4 {
        bail!("rfft2 expects rank-4 input, got {:?}", dims)
    }
    let (batch, channels, height, width) = (dims[0], dims[1], dims[2], dims[3]);
    let w_half = width / 2 + 1;
    let src = match storage {
        CpuStorage::F32(vs) => vs,
        _ => bail!("rfft2 only supports f32 inputs on cpu"),
    };
    let (start, end) = layout
        .contiguous_offsets()
        .ok_or_else(|| candle_core::Error::RequiresContiguous { op: "rfft2" }.bt())?;
    let src = &src[start..end];

    let mut planner = FftPlanner::<f32>::new();
    let fft_w = planner.plan_fft_forward(width);
    let fft_h = planner.plan_fft_forward(height);

    let mut row_buffer = vec![Complex32::default(); width * height];
    let mut col_buffer = vec![Complex32::default(); height];
    let mut dst = vec![0f32; batch * channels * height * w_half * 2];

    let plane_in_stride = height * width;
    let plane_out_stride = height * w_half * 2;
    for bc in 0..(batch * channels) {
        let plane = &src[bc * plane_in_stride..(bc + 1) * plane_in_stride];
        row_buffer
            .iter_mut()
            .zip(plane.iter())
            .for_each(|(dst, &v)| *dst = Complex32::new(v, 0.0));

        for row in row_buffer.chunks_exact_mut(width) {
            fft_w.process(row);
        }

        for x in 0..width {
            for (dst, src) in col_buffer
                .iter_mut()
                .zip(row_buffer.iter().skip(x).step_by(width))
            {
                *dst = *src;
            }
            fft_h.process(&mut col_buffer);
            for (dst, src) in row_buffer
                .iter_mut()
                .skip(x)
                .step_by(width)
                .zip(col_buffer.iter())
            {
                *dst = *src;
            }
        }

        for (y, row) in row_buffer.chunks_exact(width).enumerate() {
            let out_row = &mut dst[bc * plane_out_stride + y * w_half * 2
                ..bc * plane_out_stride + (y + 1) * w_half * 2];
            for (x, c) in row.iter().enumerate().take(w_half) {
                let base = x * 2;
                out_row[base] = c.re;
                out_row[base + 1] = c.im;
            }
        }
    }

    let shape = Shape::from(vec![batch, channels, height, w_half, 2]);
    Ok((CpuStorage::F32(dst), shape))
}

pub fn irfft2(storage: &CpuStorage, layout: &Layout, width: usize) -> Result<(CpuStorage, Shape)> {
    let dims = layout.dims();
    if dims.len() != 5 || dims[4] != 2 {
        bail!("irfft2 expects spectrum shaped [batch, channels, height, width/2+1, 2]")
    }
    let (batch, channels, height, w_half) = (dims[0], dims[1], dims[2], dims[3]);
    let src = match storage {
        CpuStorage::F32(vs) => vs,
        _ => bail!("irfft2 only supports f32 inputs on cpu"),
    };
    let (start, end) = layout
        .contiguous_offsets()
        .ok_or_else(|| candle_core::Error::RequiresContiguous { op: "irfft2" }.bt())?;
    let src = &src[start..end];

    let mut planner = FftPlanner::<f32>::new();
    let ifft_w = planner.plan_fft_inverse(width);
    let ifft_h = planner.plan_fft_inverse(height);

    let mut buffer = vec![Complex32::default(); height * width];
    let mut col_buffer = vec![Complex32::default(); height];
    let mut dst = vec![0f32; batch * channels * height * width];

    let plane_in_stride = height * w_half * 2;
    let plane_out_stride = height * width;
    for bc in 0..(batch * channels) {
        let plane = &src[bc * plane_in_stride..(bc + 1) * plane_in_stride];
        for (y, row) in plane.chunks_exact(w_half * 2).enumerate() {
            let dst_row = &mut buffer[y * width..(y + 1) * width];
            for (x, c) in dst_row.iter_mut().enumerate().take(w_half) {
                let base = x * 2;
                *c = Complex32::new(row[base], row[base + 1]);
            }
            for x in 1..w_half {
                let mirror = width - x;
                dst_row[mirror] = dst_row[x].conj();
            }
        }

        for row in buffer.chunks_exact_mut(width) {
            ifft_w.process(row);
        }

        for x in 0..width {
            for (dst, src) in col_buffer
                .iter_mut()
                .zip(buffer.iter().skip(x).step_by(width))
            {
                *dst = *src;
            }
            ifft_h.process(&mut col_buffer);
            for (dst, src) in buffer
                .iter_mut()
                .skip(x)
                .step_by(width)
                .zip(col_buffer.iter())
            {
                *dst = *src;
            }
        }

        let out_plane = &mut dst[bc * plane_out_stride..(bc + 1) * plane_out_stride];
        for (out, val) in out_plane.iter_mut().zip(buffer.iter()) {
            *out = val.re;
        }
    }

    let shape = Shape::from(vec![batch, channels, height, width]);
    Ok((CpuStorage::F32(dst), shape))
}
