use std::fmt;

#[derive(Debug, Clone)]
pub struct GnssDops {
    #[allow(dead_code)]
    pub pgn: u32,
    #[allow(dead_code)]
    pub sid: u8,
    pub desired_mode: GnssMode,
    pub actual_mode: GnssMode,
    pub hdop: Option<f64>,
    pub vdop: Option<f64>,
    pub tdop: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum GnssMode {
    OneDimensional,
    TwoDimensional,
    ThreeDimensional,
    Auto,
    Unknown(u8),
}

impl fmt::Display for GnssMode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", match self {
            GnssMode::OneDimensional => "1D",
            GnssMode::TwoDimensional => "2D",
            GnssMode::ThreeDimensional => "3D",
            GnssMode::Auto => "Auto",
            GnssMode::Unknown(_) => "Unknown",
        })
    }
}

fn parse_mode(raw: u8) -> GnssMode {
    match raw {
        0 => GnssMode::OneDimensional,
        1 => GnssMode::TwoDimensional,
        2 => GnssMode::ThreeDimensional,
        3 => GnssMode::Auto,
        v => GnssMode::Unknown(v),
    }
}

impl GnssDops {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 8 {
            return None;
        }
        let hdop_raw = i16::from_le_bytes([data[2], data[3]]);
        let vdop_raw = i16::from_le_bytes([data[4], data[5]]);
        let tdop_raw = i16::from_le_bytes([data[6], data[7]]);
        Some(Self {
            pgn: 129539,
            sid: data[0],
            desired_mode: parse_mode(data[1] & 0x07),
            actual_mode: parse_mode((data[1] >> 3) & 0x07),
            hdop: if hdop_raw >= i16::MAX - 1 { None } else { Some(hdop_raw as f64 * 0.01) },
            vdop: if vdop_raw >= i16::MAX - 1 { None } else { Some(vdop_raw as f64 * 0.01) },
            tdop: if tdop_raw >= i16::MAX - 1 { None } else { Some(tdop_raw as f64 * 0.01) },
        })
    }
}

impl fmt::Display for GnssDops {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "      GNSS DOPs: desired={} actual={}",
            self.desired_mode, self.actual_mode)?;
        if let Some(hdop) = self.hdop {
            write!(f, " | HDOP={:.2}", hdop)?;
        }
        if let Some(vdop) = self.vdop {
            write!(f, " | VDOP={:.2}", vdop)?;
        }
        if let Some(tdop) = self.tdop {
            write!(f, " | TDOP={:.2}", tdop)?;
        }
        Ok(())
    }
}
