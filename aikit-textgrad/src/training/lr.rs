//! Cosine learning-rate schedule.

use std::f64::consts::PI;

/// Compute the textual learning rate for the given epoch.
///
/// Formula: `max(1, round(lr_0 * 0.5 * (1 + cos(π * epoch / n_epochs))))`
///
/// `epoch` is 0-indexed. At epoch 0 the LR equals `lr_0`; at epoch `n_epochs` it is 1.
pub fn compute_lr(epoch: u32, n_epochs: u32, lr_0: u32) -> usize {
    let e = epoch as f64;
    let n = n_epochs as f64;
    let l = lr_0 as f64;
    let val = l * 0.5 * (1.0 + (PI * e / n).cos());
    (val.round() as usize).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    // AC30: 3-epoch run, lr_0=4 → [4, 3, 1]
    #[test]
    fn test_lr_cosine_decay_3_epochs() {
        assert_eq!(compute_lr(0, 3, 4), 4, "epoch 0 should equal lr_0");
        assert_eq!(compute_lr(1, 3, 4), 3, "epoch 1 should be 3");
        assert_eq!(compute_lr(2, 3, 4), 1, "epoch 2 should be 1");
    }

    #[test]
    fn test_lr_minimum_is_one() {
        // lr is always >= 1 even when cos gives a very small value
        for epoch in 0u32..10 {
            assert!(compute_lr(epoch, 10, 1) >= 1);
        }
    }

    #[test]
    fn test_lr_epoch_zero_equals_lr_0() {
        assert_eq!(compute_lr(0, 5, 8), 8);
    }

    #[test]
    fn test_lr_large_lr0() {
        // Should not panic with large values
        let lr = compute_lr(0, 10, 1000);
        assert_eq!(lr, 1000);
    }
}
