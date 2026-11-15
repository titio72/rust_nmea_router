use std::cmp::Ordering;
use std::fmt;

/// The 64-bit NAME of a NMEA2000 node (ISO 11783-5), as carried by PGN 60928.
///
/// Bit layout, LSB first: unique number 0-20, manufacturer 21-31, device instance lower 32-34,
/// device instance upper 35-39, device function 40-47, reserved 48, device class 49-55,
/// system instance 56-59, industry group 60-62, arbitrary-address-capable 63.
///
/// Fields keep their raw wire values (no "not available" filtering): the NAME is an identity
/// and arbitration key, so it must round-trip bit for bit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct N2kName {
    pub unique_number: u32,
    pub manufacturer_code: u16,
    pub device_instance_lower: u8,
    pub device_instance_upper: u8,
    pub device_function: u8,
    pub device_class: u8,
    pub system_instance: u8,
    pub industry_group: u8,
    pub arbitrary_address_capable: bool,
}

impl N2kName {
    pub fn from_u64(raw: u64) -> Self {
        Self {
            unique_number: (raw & 0x1F_FFFF) as u32,
            manufacturer_code: ((raw >> 21) & 0x7FF) as u16,
            device_instance_lower: ((raw >> 32) & 0x7) as u8,
            device_instance_upper: ((raw >> 35) & 0x1F) as u8,
            device_function: ((raw >> 40) & 0xFF) as u8,
            device_class: ((raw >> 49) & 0x7F) as u8,
            system_instance: ((raw >> 56) & 0xF) as u8,
            industry_group: ((raw >> 60) & 0x7) as u8,
            arbitrary_address_capable: (raw >> 63) & 1 == 1,
        }
    }

    /// Encodes the NAME; the reserved bit 48 is always set to 1. Out-of-range field values are masked.
    pub fn to_u64(&self) -> u64 {
        (self.unique_number as u64 & 0x1F_FFFF)
            | ((self.manufacturer_code as u64 & 0x7FF) << 21)
            | ((self.device_instance_lower as u64 & 0x7) << 32)
            | ((self.device_instance_upper as u64 & 0x1F) << 35)
            | ((self.device_function as u64) << 40)
            | (1u64 << 48)
            | ((self.device_class as u64 & 0x7F) << 49)
            | ((self.system_instance as u64 & 0xF) << 56)
            | ((self.industry_group as u64 & 0x7) << 60)
            | ((self.arbitrary_address_capable as u64) << 63)
    }
}

/// Address arbitration order: the lower NAME wins, so a lower value is a higher priority.
impl Ord for N2kName {
    fn cmp(&self, other: &Self) -> Ordering {
        self.to_u64().cmp(&other.to_u64())
    }
}

impl PartialOrd for N2kName {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for N2kName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016X}", self.to_u64())
    }
}

/// PGN 60928 ISO Address Claim: 8 bytes, the NAME as u64 little-endian.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressClaim {
    pub name: N2kName,
}

impl AddressClaim {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        let raw: [u8; 8] = data.get(..8)?.try_into().ok()?;
        Some(Self {
            name: N2kName::from_u64(u64::from_le_bytes(raw)),
        })
    }

    pub fn to_bytes(&self) -> [u8; 8] {
        self.name.to_u64().to_le_bytes()
    }
}

impl fmt::Display for AddressClaim {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let n = &self.name;
        write!(
            f,
            "Address Claim: NAME {} (unique {}, manufacturer {}, function {}, class {}, instance {}/{}, system instance {}, industry {})",
            n, n.unique_number, n.manufacturer_code, n.device_function, n.device_class,
            n.device_instance_upper, n.device_instance_lower, n.system_instance, n.industry_group
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_round_trips_router_name() {
        let n = N2kName { unique_number: 271828, manufacturer_code: 2046, device_instance_lower: 0,
            device_instance_upper: 0, device_function: 130, device_class: 25, system_instance: 15,
            industry_group: 4, arbitrary_address_capable: true };
        assert_eq!(N2kName::from_u64(n.to_u64()), n);
        let claim = AddressClaim { name: n };
        assert_eq!(AddressClaim::from_bytes(&claim.to_bytes()).unwrap().name, n);
    }

    #[test]
    fn name_field_positions() {
        let raw: u64 = (1u64 << 63) | (4u64 << 60) | (15u64 << 56) | (25u64 << 49) | (130u64 << 40) | (2046u64 << 21) | 271828;
        let n = N2kName::from_u64(raw);
        assert_eq!((n.industry_group, n.system_instance, n.device_class, n.device_function, n.manufacturer_code, n.unique_number),
                   (4, 15, 25, 130, 2046, 271828));
    }

    #[test]
    fn router_name_loses_to_typical_device() {
        // EV-1-like: industry 4, system instance 0, class 60, AAC 1 -> lower NAME wins
        let ev1 = N2kName { unique_number: 1, manufacturer_code: 1851, device_instance_lower: 0, device_instance_upper: 0,
            device_function: 140, device_class: 60, system_instance: 0, industry_group: 4, arbitrary_address_capable: true };
        let router = N2kName { system_instance: 15, device_class: 25, device_function: 130, manufacturer_code: 2046, unique_number: 271828, ..ev1 };
        assert!(ev1 < router);
    }

    #[test]
    fn short_claim_rejected() {
        assert!(AddressClaim::from_bytes(&[0u8; 7]).is_none());
    }

    #[test]
    fn reserved_bit_is_encoded_as_one() {
        let n = N2kName::from_u64(0);
        assert_eq!(n.to_u64(), 1u64 << 48);
    }

    #[test]
    fn instances_round_trip_at_limits() {
        let n = N2kName::from_u64(0);
        let n = N2kName { device_instance_lower: 7, device_instance_upper: 31, ..n };
        let back = N2kName::from_u64(n.to_u64());
        assert_eq!((back.device_instance_lower, back.device_instance_upper), (7, 31));
        assert_eq!(back.unique_number, 0);
        assert_eq!(back.device_function, 0);
    }

    // canboat database/pgns/060928-isoAddressClaim.yaml, samples 1-3
    #[test]
    fn canboat_samples() {
        let s1 = [0x58, 0x87, 0xb0, 0x2f, 0x00, 0x82, 0xaa, 0xc0];
        let n = AddressClaim::from_bytes(&s1).unwrap().name;
        assert_eq!((n.unique_number, n.manufacturer_code, n.device_instance_lower, n.device_instance_upper), (1083224, 381, 0, 0));
        assert_eq!((n.device_function, n.device_class, n.system_instance, n.industry_group, n.arbitrary_address_capable), (130, 85, 0, 4, true));

        let s2 = [0xae, 0x2c, 0x69, 0xe7, 0x19, 0x14, 0x32, 0xc1];
        let n = AddressClaim::from_bytes(&s2).unwrap().name;
        assert_eq!((n.unique_number, n.manufacturer_code, n.device_instance_lower, n.device_instance_upper), (601262, 1851, 1, 3));
        assert_eq!((n.device_function, n.device_class, n.system_instance, n.industry_group, n.arbitrary_address_capable), (20, 25, 1, 4, true));

        let s3 = [0xfb, 0xdb, 0x70, 0x22, 0xff, 0x9b, 0x50, 0xc0];
        let n = AddressClaim::from_bytes(&s3).unwrap().name;
        assert_eq!((n.unique_number, n.manufacturer_code, n.device_instance_lower, n.device_instance_upper), (1104891, 275, 7, 31));
        assert_eq!((n.device_function, n.device_class, n.system_instance, n.industry_group, n.arbitrary_address_capable), (155, 40, 0, 4, true));
    }

    #[test]
    fn extra_bytes_ignored() {
        let mut data = vec![0x58, 0x87, 0xb0, 0x2f, 0x00, 0x82, 0xaa, 0xc0];
        data.push(0xFF);
        assert!(AddressClaim::from_bytes(&data).is_some());
    }
}
