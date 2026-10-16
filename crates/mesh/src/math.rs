pub trait Real: Copy {
    fn sin_of(self) -> Self;
    fn cos_of(self) -> Self;
    fn sin_cos_of(self) -> (Self, Self);
    fn tan_of(self) -> Self;
    fn asin_of(self) -> Self;
    fn acos_of(self) -> Self;
    fn atan_of(self) -> Self;
    fn atan2_of(self, x: Self) -> Self;
    fn tanh_of(self) -> Self;
    fn exp_of(self) -> Self;
    fn ln_of(self) -> Self;
    fn hypot_of(self, other: Self) -> Self;
    fn powf_of(self, power: Self) -> Self;
}

macro_rules! real {
    ($t:ty, $sin:ident, $cos:ident, $sincos:ident, $tan:ident, $asin:ident, $acos:ident, $atan:ident, $atan2:ident, $tanh:ident, $exp:ident, $ln:ident, $hypot:ident, $pow:ident) => {
        impl Real for $t {
            fn sin_of(self) -> $t {
                libm::$sin(self)
            }
            fn cos_of(self) -> $t {
                libm::$cos(self)
            }
            fn sin_cos_of(self) -> ($t, $t) {
                libm::$sincos(self)
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
            fn tanh_of(self) -> $t {
                libm::$tanh(self)
            }
            fn exp_of(self) -> $t {
                libm::$exp(self)
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
    f64, sin, cos, sincos, tan, asin, acos, atan, atan2, tanh, exp, log, hypot, pow
);
real!(
    f32, sinf, cosf, sincosf, tanf, asinf, acosf, atanf, atan2f, tanhf, expf, logf, hypotf, powf
);
