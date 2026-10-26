/// NMEA2000 "no data" sentinels.
///
/// Every numeric field gives up the top three raw values of its range to carry a status instead
/// of a reading: the maximum means "data not available", maximum − 1 "out of range", and
/// maximum − 2 is reserved. A signed field uses the top of its *positive* range (0x7FFF for an
/// i16), not the all-ones pattern, which is a legitimate negative value.
///
/// Decoders route every raw numeric read through [`Available::available`] so that none of these
/// patterns ever surfaces as a number (a wind speed of 655.35 m/s, a latitude of 214°, ...).
pub(crate) trait Available: Sized {
    /// The raw value, or `None` when it is one of the field's three reserved values.
    fn available(self) -> Option<Self>;
}

macro_rules! impl_available {
    ($($t:ty),*) => {$(
        impl Available for $t {
            fn available(self) -> Option<Self> {
                (self < <$t>::MAX - 2).then_some(self)
            }
        }
    )*};
}

impl_available!(u8, u16, u32, i8, i16, i32, i64);

/// A latitude in degrees, or `None` when it is unavailable or outside ±90°.
///
/// The range check matters for AIS, where a transponder without a fix reports 91° and gateways
/// pass it through unchanged.
pub(crate) fn latitude(degrees: Option<f64>) -> Option<f64> {
    degrees.filter(|d| d.abs() <= 90.0)
}

/// A longitude in degrees, or `None` when it is unavailable or outside ±180° (AIS uses 181°).
pub(crate) fn longitude(degrees: Option<f64>) -> Option<f64> {
    degrees.filter(|d| d.abs() <= 180.0)
}

/// Formats an optional reading for `Display` impls: `precision` decimals, or "N/A".
pub(crate) fn fmt_opt(value: Option<f64>, precision: usize) -> String {
    value.map_or_else(|| "N/A".to_string(), |v| format!("{:.*}", precision, v))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unsigned_top_three_values_are_unavailable() {
        assert_eq!(0xFFFFu16.available(), None); // not available
        assert_eq!(0xFFFEu16.available(), None); // out of range
        assert_eq!(0xFFFDu16.available(), None); // reserved
        assert_eq!(0xFFFCu16.available(), Some(0xFFFC));
        assert_eq!(0u16.available(), Some(0));
        assert_eq!(0xFFu8.available(), None);
        assert_eq!(0xFCu8.available(), Some(0xFC));
        assert_eq!(u32::MAX.available(), None);
    }

    #[test]
    fn test_signed_sentinels_sit_at_top_of_positive_range() {
        assert_eq!(i16::MAX.available(), None);
        assert_eq!((i16::MAX - 2).available(), None);
        assert_eq!((i16::MAX - 3).available(), Some(i16::MAX - 3));
        // All-ones is -1, a real reading
        assert_eq!((-1i16).available(), Some(-1));
        assert_eq!(i16::MIN.available(), Some(i16::MIN));
        assert_eq!(0x7Fi8.available(), None);
        assert_eq!(i32::MAX.available(), None);
        assert_eq!(i64::MAX.available(), None);
    }

    #[test]
    fn test_latitude_longitude_range() {
        assert_eq!(latitude(Some(43.5)), Some(43.5));
        assert_eq!(latitude(Some(-90.0)), Some(-90.0));
        assert_eq!(latitude(Some(91.0)), None);
        assert_eq!(latitude(None), None);
        assert_eq!(longitude(Some(-180.0)), Some(-180.0));
        assert_eq!(longitude(Some(181.0)), None);
    }
}
