use glam::{DMat4, DQuat, DVec3};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trs {
    pub translation: [f64; 3],
    pub rotation: [f64; 4],
    pub scale: [f64; 3],
}

impl Default for Trs {
    fn default() -> Trs {
        Trs::IDENTITY
    }
}

impl Trs {
    pub const IDENTITY: Trs = Trs {
        translation: [0.0; 3],
        rotation: [0.0, 0.0, 0.0, 1.0],
        scale: [1.0; 3],
    };

    pub fn quat(&self) -> DQuat {
        DQuat::from_array(self.rotation).normalize()
    }
}

pub fn trs_from_matrix(m: DMat4) -> Trs {
    let (scale, rotation, translation) = m.to_scale_rotation_translation();
    Trs {
        translation: translation.to_array(),
        rotation: rotation.normalize().to_array(),
        scale: scale.to_array(),
    }
}

pub fn matrix(trs: &Trs) -> DMat4 {
    DMat4::from_scale_rotation_translation(
        DVec3::from(trs.scale),
        trs.quat(),
        DVec3::from(trs.translation),
    )
}

pub fn sample(keys: &[Trs], fps: u32, time: f64) -> Trs {
    let Some(last) = keys.len().checked_sub(1) else {
        return Trs::IDENTITY;
    };
    let at = time * f64::from(fps.max(1));
    let at = if at.is_finite() {
        at.clamp(0.0, last as f64)
    } else {
        0.0
    };
    let low = (at.floor() as usize).min(last);
    let high = (low + 1).min(last);
    let s = at - low as f64;
    let (a, b) = (&keys[low], &keys[high]);
    if low == high || s == 0.0 {
        return *a;
    }
    let lerp = |x: [f64; 3], y: [f64; 3]| DVec3::from(x).lerp(DVec3::from(y), s).to_array();
    Trs {
        translation: lerp(a.translation, b.translation),
        rotation: a.quat().slerp(b.quat(), s).normalize().to_array(),
        scale: lerp(a.scale, b.scale),
    }
}
