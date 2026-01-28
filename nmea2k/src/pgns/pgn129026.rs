use std::fmt;

#[derive(Debug, Clone)]
pub struct CogSogRapidUpdate {
    #[allow(dead_code)]
    pub pgn: u32,
    #[allow(dead_code)]
    sid: u8,
    pub cog_reference: bool, // true = True, false = Magnetic
    pub cog: f64, // radians
    pub sog: f64, // m/s
}

impl CogSogRapidUpdate {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 8 {
            return None;
        }
        Some(Self {
            pgn: 129026,
            sid: data[0],
            cog_reference: (data[1] & 0x03) == 0,
            cog: u16::from_le_bytes([data[2], data[3]]) as f64 * 0.0001,
            sog: u16::from_le_bytes([data[4], data[5]]) as f64 * 0.01,
        })
    }

    pub fn sog_knots(&self) -> f64 {
        self.sog * 1.94384
    }

    pub fn cog_degrees(&self) -> f64 {
        self.cog.to_degrees()
    }
}

impl fmt::Display for CogSogRapidUpdate {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "      COG: {:.2}° ({}) | SOG: {:.2} m/s ({:.2} knots)",
            self.cog.to_degrees(),
            if self.cog_reference { "True" } else { "Mag" },
            self.sog,
            self.sog * 1.94384
        )
    }
}
