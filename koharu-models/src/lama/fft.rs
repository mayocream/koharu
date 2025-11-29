use candle_core::{DType, Tensor};

pub(super) fn move_axis_last(
    t: &Tensor,
    axis: usize,
) -> candle_core::Result<(Tensor, Vec<usize>, Vec<usize>)> {
    let rank = t.rank();
    let mut perm: Vec<usize> = (0..rank).collect();
    let moved = perm.remove(axis);
    perm.push(moved);
    let mut inv_perm = vec![0usize; rank];
    for (i, &p) in perm.iter().enumerate() {
        inv_perm[p] = i;
    }
    Ok((t.permute(perm.clone())?, perm, inv_perm))
}

fn is_power_of_two(n: usize) -> bool {
    n != 0 && (n & (n - 1)) == 0
}

fn bit_reverse_indices(len: usize) -> Vec<i64> {
    let bits = (usize::BITS - (len.leading_zeros() + 1)) as u32;
    (0..len)
        .map(|i| {
            let mut v = i;
            let mut r = 0usize;
            for _ in 0..bits {
                r = (r << 1) | (v & 1);
                v >>= 1;
            }
            r as i64
        })
        .collect()
}

pub(super) fn fft_axis_power2(
    re: &Tensor,
    im: &Tensor,
    inverse: bool,
) -> candle_core::Result<(Tensor, Tensor)> {
    let (outer, len) = re.dims2()?;
    if len == 1 {
        return Ok((re.clone(), im.clone()));
    }

    let idx = Tensor::from_vec(bit_reverse_indices(len), len, re.device())?;
    let mut re = re.index_select(&idx, 1)?;
    let mut im = im.index_select(&idx, 1)?;

    let mut step = 2;
    while step <= len {
        let half = step / 2;
        let blocks = len / step;
        let angles = (0..half)
            .map(|k| 2.0f32 * std::f32::consts::PI * k as f32 / step as f32)
            .collect::<Vec<_>>();
        let cos = Tensor::from_vec(
            angles.iter().map(|a| a.cos()).collect::<Vec<_>>(),
            (1, 1, half),
            re.device(),
        )?;
        let sign = if inverse { 1.0f32 } else { -1.0f32 };
        let sin = Tensor::from_vec(
            angles.iter().map(|a| sign * a.sin()).collect::<Vec<_>>(),
            (1, 1, half),
            re.device(),
        )?;

        let re_blocks = re.reshape((outer, blocks, step))?;
        let im_blocks = im.reshape((outer, blocks, step))?;

        let even_re = re_blocks.narrow(2, 0, half)?;
        let odd_re = re_blocks.narrow(2, half, half)?;
        let even_im = im_blocks.narrow(2, 0, half)?;
        let odd_im = im_blocks.narrow(2, half, half)?;

        let cos_b = cos.broadcast_as(odd_re.shape())?;
        let sin_b = sin.broadcast_as(odd_re.shape())?;

        let t_re = ((&odd_re * &cos_b)? - (&odd_im * &sin_b)?)?;
        let t_im = ((&odd_im * &cos_b)? + (&odd_re * &sin_b)?)?;

        let out_even_re = (&even_re + &t_re)?;
        let out_even_im = (&even_im + &t_im)?;
        let out_odd_re = (&even_re - &t_re)?;
        let out_odd_im = (&even_im - &t_im)?;

        let re_new = Tensor::cat(&[&out_even_re, &out_odd_re], 2)?;
        let im_new = Tensor::cat(&[&out_even_im, &out_odd_im], 2)?;

        re = re_new.reshape((outer, len))?;
        im = im_new.reshape((outer, len))?;

        step *= 2;
    }

    let scale = Tensor::full(1.0f32 / (len as f32).sqrt(), (outer, len), re.device())?;
    re = (re * &scale)?;
    im = (im * &scale)?;
    Ok((re, im))
}

pub(super) fn dft_axis(
    re: &Tensor,
    im: &Tensor,
    axis: usize,
    inverse: bool,
) -> candle_core::Result<(Tensor, Tensor)> {
    let (re_p, perm, inv_perm) = move_axis_last(re, axis)?;
    let im_p = im.permute(perm.clone())?;
    let dims = re_p.dims().to_vec();
    let len = *dims.last().unwrap();
    let outer = re.elem_count() / len;
    let re_flat = re_p.reshape((outer, len))?;
    let im_flat = im_p.reshape((outer, len))?;

    let (re_fft, im_fft) = fft_axis_power2(&re_flat, &im_flat, inverse)?;

    let re_back = re_fft.reshape(dims.clone())?;
    let im_back = im_fft.reshape(dims)?;
    let re_final = re_back.permute(inv_perm.clone())?;
    let im_final = im_back.permute(inv_perm)?;
    Ok((re_final, im_final))
}

fn next_pow2(n: usize) -> usize {
    if is_power_of_two(n) {
        n
    } else {
        1usize << (usize::BITS - (n - 1).leading_zeros())
    }
}

fn pad_to_pow2(xs: &Tensor) -> candle_core::Result<(Tensor, usize, usize)> {
    let (_b, _c, h, w) = xs.dims4()?;
    let h2 = next_pow2(h);
    let w2 = next_pow2(w);
    let pad_h = h2 - h;
    let pad_w = w2 - w;
    let xs = xs
        .pad_with_zeros(3, 0, pad_w)?
        .pad_with_zeros(2, 0, pad_h)?;
    Ok((xs, h2, w2))
}

pub(super) fn rfft2_power2(
    xs: &Tensor,
) -> candle_core::Result<(Tensor, Tensor, usize, usize, usize, usize)> {
    let (b, c, h, w) = xs.dims4()?;
    let (padded, h2, w2) = pad_to_pow2(xs)?;
    let re0 = padded.to_dtype(DType::F32)?;
    let im0 = Tensor::zeros_like(&re0)?;
    let (re_w, im_w) = dft_axis(&re0, &im0, 3, false)?;
    let (mut re_hw, mut im_hw) = dft_axis(&re_w, &im_w, 2, false)?;
    let w_half = w2 / 2 + 1;
    re_hw = re_hw.narrow(3, 0, w_half)?;
    im_hw = im_hw.narrow(3, 0, w_half)?;
    re_hw = re_hw.reshape((b, c, h2, w_half))?;
    im_hw = im_hw.reshape((b, c, h2, w_half))?;
    Ok((re_hw, im_hw, h2, w2, h, w))
}

pub(super) fn irfft2_power2(
    re_half: &Tensor,
    im_half: &Tensor,
    h_pad: usize,
    w_pad: usize,
    h_orig: usize,
    w_orig: usize,
) -> candle_core::Result<Tensor> {
    let (b, c, _h, w_half) = re_half.dims4()?;
    let mirror_len = if w_pad % 2 == 0 {
        w_half - 2
    } else {
        w_half - 1
    };
    let tail_re = re_half.narrow(3, 1, mirror_len)?.contiguous()?.flip(&[3])?;
    let tail_im = im_half
        .narrow(3, 1, mirror_len)?
        .contiguous()?
        .flip(&[3])?
        .neg()?;
    let re_full = Tensor::cat(&[re_half, &tail_re], 3)?;
    let im_full = Tensor::cat(&[im_half, &tail_im], 3)?;
    let (re_h, im_h) = dft_axis(&re_full, &im_full, 3, true)?;
    let (re, _im) = dft_axis(&re_h, &im_h, 2, true)?;
    let re = re.reshape((b, c, h_pad, w_pad))?;
    re.narrow(2, 0, h_orig)?.narrow(3, 0, w_orig)?.contiguous()
}
