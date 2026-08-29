//! 再現可能な乱数。seed は `PachinkoState` が持ち、セーブとシミュレーターが
//! 同じ系列を再生できるようにする。
//!
//! 盤面・釘・物理・抽選が同じ生成器を共有するので、ここに閉じる。各ドメインが
//! 独自の RNG を持つと、同じ seed からホールを復元できなくなる。

/// xorshift32。0 は不動点なので、落ちたら別の値へ逃がす。
pub(super) fn rng_next(seed: &mut u32) -> u32 {
    let mut x = *seed;
    if x == 0 {
        x = 0xDEAD_BEEF;
    }
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *seed = x;
    x
}

pub(super) fn rng_below(seed: &mut u32, bound: u32) -> u32 {
    if bound == 0 {
        return 0;
    }
    rng_next(seed) % bound
}

pub(super) fn rand01(seed: &mut u32) -> f64 {
    (rng_next(seed) as f64) / (u32::MAX as f64)
}

pub(super) fn rand_range(seed: &mut u32, lo: f64, hi: f64) -> f64 {
    lo + (hi - lo) * rand01(seed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rng_next_escapes_the_zero_fixed_point() {
        let mut seed = 0;
        let a = rng_next(&mut seed);
        let b = rng_next(&mut seed);
        assert_ne!(a, 0);
        assert_ne!(b, 0);
        assert_ne!(a, b);
    }

    #[test]
    fn rng_below_zero_bound_is_zero() {
        let mut seed = 1;
        assert_eq!(rng_below(&mut seed, 0), 0);
    }
}
