//! Pointer math for percentage sliders.
pub(crate) fn value_at(x: f32, left: f32, right: f32, min: u8, max: u8) -> u8 {
    if !x.is_finite() || right <= left {
        return min;
    }
    let fraction = ((x - left) / (right - left)).clamp(0.0, 1.0);
    min + (fraction * max.saturating_sub(min) as f32).round() as u8
}

pub(crate) fn grab_offset(x: f32, left: f32, right: f32, fraction: f32, radius: f32) -> f32 {
    let offset = x - (left + (right - left) * fraction);
    if offset.abs() <= radius {
        offset
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn track_clicks_and_thumb_drags_clamp_without_wrapping_or_jumping() {
        assert_eq!(value_at(-200., 10., 210., 0, 100), 0);
        assert_eq!(value_at(500., 10., 210., 0, 100), 100);
        assert_eq!(value_at(84., 10., 210., 0, 100), 37);
        let grab = grab_offset(115., 10., 210., 0.5, 8.);
        assert_eq!(value_at(115. - grab, 10., 210., 0, 100), 50);
        assert_eq!(value_at(155. - grab, 10., 210., 0, 100), 70);
        assert_eq!(grab_offset(190., 10., 210., 0.5, 8.), 0.);
        assert_eq!(value_at(110., 10., 210., 40, 100), 70);
    }
}
