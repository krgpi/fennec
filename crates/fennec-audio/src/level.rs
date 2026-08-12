pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f32 = samples.iter().map(|s| s * s).sum();
    (sum / samples.len() as f32).sqrt()
}

pub fn rms_to_level(rms: f32) -> f32 {
    if rms <= 0.0 {
        return 0.0;
    }
    let db = 20.0 * rms.log10();
    let min_db = -50.0f32;
    ((db - min_db) / -min_db).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rms_of_silence_is_zero() {
        assert_eq!(rms(&[0.0; 128]), 0.0);
        assert_eq!(rms(&[]), 0.0);
    }

    #[test]
    fn rms_of_full_scale_square_is_one() {
        let samples = [1.0f32, -1.0, 1.0, -1.0];
        assert!((rms(&samples) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn level_mapping_matches_swift() {
        assert_eq!(rms_to_level(0.0), 0.0);
        assert!((rms_to_level(1.0) - 1.0).abs() < 1e-6);
        let level = rms_to_level(0.0316228);
        assert!((level - 0.4).abs() < 0.01);
    }
}
