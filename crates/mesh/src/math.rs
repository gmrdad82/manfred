pub trait Real: Copy {
    fn sin_cos_of(self) -> (Self, Self);
    fn sin_of(self) -> Self;
    fn cos_of(self) -> Self;
    fn tan_of(self) -> Self;
    fn asin_of(self) -> Self;
    fn acos_of(self) -> Self;
    fn atan_of(self) -> Self;
    fn atan2_of(self, x: Self) -> Self;
    fn ln_of(self) -> Self;
    fn hypot_of(self, other: Self) -> Self;
    fn powf_of(self, power: Self) -> Self;
}

macro_rules! real {
    ($t:ty, $sincos:ident, $sin:ident, $cos:ident, $tan:ident, $asin:ident, $acos:ident, $atan:ident, $atan2:ident, $ln:ident, $hypot:ident, $pow:ident) => {
        impl Real for $t {
            fn sin_cos_of(self) -> ($t, $t) {
                libm::$sincos(self)
            }
            fn sin_of(self) -> $t {
                libm::$sin(self)
            }
            fn cos_of(self) -> $t {
                libm::$cos(self)
            }
            fn tan_of(self) -> $t {
                libm::$tan(self)
            }
            fn asin_of(self) -> $t {
                libm::$asin(self)
            }
            fn acos_of(self) -> $t {
                libm::$acos(self)
            }
            fn atan_of(self) -> $t {
                libm::$atan(self)
            }
            fn atan2_of(self, x: $t) -> $t {
                libm::$atan2(self, x)
            }
            fn ln_of(self) -> $t {
                libm::$ln(self)
            }
            fn hypot_of(self, other: $t) -> $t {
                libm::$hypot(self, other)
            }
            fn powf_of(self, power: $t) -> $t {
                libm::$pow(self, power)
            }
        }
    };
}

real!(
    f64, sincos, sin, cos, tan, asin, acos, atan, atan2, log, hypot, pow
);
real!(
    f32, sincosf, sinf, cosf, tanf, asinf, acosf, atanf, atan2f, logf, hypotf, powf
);

pub fn sin_cos<T: Real>(x: T) -> (T, T) {
    x.sin_cos_of()
}

pub fn sin<T: Real>(x: T) -> T {
    x.sin_of()
}

pub fn cos<T: Real>(x: T) -> T {
    x.cos_of()
}

pub fn tan<T: Real>(x: T) -> T {
    x.tan_of()
}

pub fn asin<T: Real>(x: T) -> T {
    x.asin_of()
}

pub fn acos<T: Real>(x: T) -> T {
    x.acos_of()
}

pub fn atan<T: Real>(x: T) -> T {
    x.atan_of()
}

pub fn atan2<T: Real>(y: T, x: T) -> T {
    y.atan2_of(x)
}

pub fn ln<T: Real>(x: T) -> T {
    x.ln_of()
}

pub fn hypot<T: Real>(x: T, y: T) -> T {
    x.hypot_of(y)
}

pub fn powf<T: Real>(x: T, power: T) -> T {
    x.powf_of(power)
}
