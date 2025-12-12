use std::sync::{Arc, Mutex};

use candle_core::{Result, Tensor, bail};

#[derive(Debug)]
struct KvCacheInner {
    k: Tensor,
    v: Tensor,
    len: usize,
    capacity: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct KvCache {
    inner: Arc<Mutex<KvCacheInner>>,
}

impl KvCache {
    pub(crate) fn new(k: &Tensor, v: &Tensor, capacity: usize, start: usize) -> Result<Self> {
        let (k, v, dims) = Self::prepare(k, v)?;
        let (b, h, seq_len, d) = dims;
        if start + seq_len > capacity {
            bail!(
                "kv cache capacity exceeded when creating cache: start {} + seq_len {} > {}",
                start,
                seq_len,
                capacity
            )
        }

        let k_cache = Tensor::zeros((b, h, capacity, d), k.dtype(), k.device())?;
        let v_cache = Tensor::zeros((b, h, capacity, d), v.dtype(), v.device())?;
        k_cache.slice_set(&k, 2, start)?;
        v_cache.slice_set(&v, 2, start)?;

        Ok(Self {
            inner: Arc::new(Mutex::new(KvCacheInner {
                k: k_cache,
                v: v_cache,
                len: start + seq_len,
                capacity,
            })),
        })
    }

    pub(crate) fn update(&mut self, k: &Tensor, v: &Tensor, start: usize) -> Result<()> {
        let (k, v, dims) = Self::prepare(k, v)?;
        let (b, h, seq_len, d) = dims;
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| candle_core::Error::Msg("kv cache lock poisoned".into()))?;
        let (cb, ch, _, cd) = guard.k.dims4()?;
        if start + seq_len > guard.capacity {
            bail!(
                "kv cache capacity exceeded: start {} + seq_len {} > {}",
                start,
                seq_len,
                guard.capacity
            )
        }
        if (b, h, d) != (cb, ch, cd) {
            bail!("kv cache shape mismatch: got ({b}, {h}, {d}) expected ({cb}, {ch}, {cd})")
        }

        guard.k.slice_set(&k, 2, start)?;
        guard.v.slice_set(&v, 2, start)?;
        guard.len = start + seq_len;
        Ok(())
    }

    pub(crate) fn view(&self) -> Result<(Tensor, Tensor)> {
        let guard = self
            .inner
            .lock()
            .map_err(|_| candle_core::Error::Msg("kv cache lock poisoned".into()))?;
        Ok((
            guard.k.narrow(2, 0, guard.len)?,
            guard.v.narrow(2, 0, guard.len)?,
        ))
    }

    fn prepare(k: &Tensor, v: &Tensor) -> Result<(Tensor, Tensor, (usize, usize, usize, usize))> {
        let k = k.contiguous()?;
        let v = v.contiguous()?;
        let kd = k.dims4()?;
        let vd = v.dims4()?;
        if kd != vd {
            bail!("k/v shape mismatch: {:?} vs {:?}", kd, vd)
        }
        if k.dtype() != v.dtype() {
            bail!("k/v dtype mismatch: {:?} vs {:?}", k.dtype(), v.dtype())
        }
        Ok((k, v, kd))
    }
}
