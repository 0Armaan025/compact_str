use super::Repr;

const FALSE: Repr = Repr::new_const("false");
const TRUE: Repr = Repr::new_const("true");

/// Defines how to _efficiently_ create a [`Repr`] from `self`
pub trait IntoRepr {
    fn into_repr(self) -> Repr;
}

impl IntoRepr for f32 {
    fn into_repr(self) -> Repr {
        #[cfg(not(all(target_arch = "powerpc64", target_pointer_width = "64")))]
        {
            let mut buf = ryu::Buffer::new();
            let s = buf.format(self);
            Repr::new(s)
        }
        // `ryu` doesn't seem to properly format `f32` on PowerPC 64-bit, so we special case that
        // to just use the `std::fmt::Display` impl
        #[cfg(all(target_arch = "powerpc64", target_pointer_width = "64"))]
        {
            use core::fmt::Write;
            let mut repr = Repr::new_const("");
            write!(&mut repr, "{}", self).expect("fmt::Display incorrectly implemented!");
            repr
        }
    }
}

impl IntoRepr for f64 {
    fn into_repr(self) -> Repr {
        let mut buf = ryu::Buffer::new();
        let s = buf.format(self);
        Repr::new(s)
    }
}

impl IntoRepr for bool {
    fn into_repr(self) -> Repr {
        if self {
            TRUE
        } else {
            FALSE
        }
    }
}

impl IntoRepr for char {
    fn into_repr(self) -> Repr {
        let mut buf = [0_u8; 4];
        Repr::new_const(self.encode_utf8(&mut buf))
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use test_strategy::proptest;

    use super::IntoRepr;

    #[test]
    fn test_into_repr_bool() {
        let t = true;
        let repr = t.into_repr();
        assert_eq!(repr.as_str(), t.to_string());

        let f = false;
        let repr = f.into_repr();
        assert_eq!(repr.as_str(), f.to_string());
    }

    #[proptest]
    #[cfg_attr(miri, ignore)]
    fn test_into_repr_char(val: char) {
        let repr = char::into_repr(val);
        prop_assert_eq!(repr.as_str(), val.to_string());
    }

    #[test]
    fn test_into_repr_f32_sanity() {
        let vals = [
            f32::MIN,
            f32::MIN_POSITIVE,
            f32::MAX,
            f32::NEG_INFINITY,
            f32::INFINITY,
        ];

        for x in &vals {
            let repr = f32::into_repr(*x);
            let roundtrip = repr.as_str().parse::<f32>().unwrap();

            assert_eq!(*x, roundtrip);
        }
    }

    #[test]
    fn test_into_repr_f32_nan() {
        let repr = f32::into_repr(f32::NAN);
        let roundtrip = repr.as_str().parse::<f32>().unwrap();
        assert!(roundtrip.is_nan());
    }

    #[proptest]
    #[cfg_attr(miri, ignore)]
    fn test_into_repr_f32(val: f32) {
        let repr = f32::into_repr(val);
        let roundtrip = repr.as_str().parse::<f32>().unwrap();

        // Note: The formatting of floats by `ryu` sometimes differs from that of `std`, so instead
        // of asserting equality with `std` we just make sure the value roundtrips

        prop_assert_eq!(val, roundtrip);
    }

    #[test]
    fn test_into_repr_f64_sanity() {
        let vals = [
            f64::MIN,
            f64::MIN_POSITIVE,
            f64::MAX,
            f64::NEG_INFINITY,
            f64::INFINITY,
        ];

        for x in &vals {
            let repr = f64::into_repr(*x);
            let roundtrip = repr.as_str().parse::<f64>().unwrap();

            assert_eq!(*x, roundtrip);
        }
    }

    #[test]
    fn test_into_repr_f64_nan() {
        let repr = f64::into_repr(f64::NAN);
        let roundtrip = repr.as_str().parse::<f64>().unwrap();
        assert!(roundtrip.is_nan());
    }

    #[proptest]
    #[cfg_attr(miri, ignore)]
    fn test_into_repr_f64(val: f64) {
        let repr = f64::into_repr(val);
        let roundtrip = repr.as_str().parse::<f64>().unwrap();

        // Note: The formatting of floats by `ryu` sometimes differs from that of `std`, so instead
        // of asserting equality with `std` we just make sure the value roundtrips

        prop_assert_eq!(val, roundtrip);
    }
}
