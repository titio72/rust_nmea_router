use std::fmt;

use super::pgn126992::SystemTime;
use super::pgn127250::VesselHeading;
use super::pgn127251::RateOfTurn;
use super::pgn127257::Attitude;
use super::pgn127488::EngineRapidUpdate;
use super::pgn128259::SpeedWaterReferenced;
use super::pgn128267::WaterDepth;
use super::pgn129025::PositionRapidUpdate;
use super::pgn129026::CogSogRapidUpdate;
use super::pgn129029::GnssPositionData;
use super::pgn129033::TimeDate;
use super::pgn130306::WindData;
use super::pgn130312::Temperature;
use super::pgn130313::Humidity;
use super::pgn130314::ActualPressure;

fn format_data_bytes(data: &[u8]) -> String {
    data.iter()
        .map(|b| format!("{:02X}", b))
        .collect::<Vec<_>>()
        .join(" ")
}

// Enum to hold any decoded message type
#[derive(Debug, Clone)]
pub enum N2kMessage {
    SystemTime(SystemTime),
    VesselHeading(VesselHeading),
    RateOfTurn(RateOfTurn),
    Attitude(Attitude),
    EngineRapidUpdate(EngineRapidUpdate),
    SpeedWaterReferenced(SpeedWaterReferenced),
    WaterDepth(WaterDepth),
    PositionRapidUpdate(PositionRapidUpdate),
    CogSogRapidUpdate(CogSogRapidUpdate),
    GnssPositionData(GnssPositionData),
    TimeDate(TimeDate),
    WindData(WindData),
    Temperature(Temperature),
    Humidity(Humidity),
    ActualPressure(ActualPressure),
    Unknown(u32, Vec<u8>),
}

impl N2kMessage {
    pub fn from_pgn(pgn: u32, data: &[u8]) -> Self {
        match pgn {
            126992 => SystemTime::from_bytes(data)
                .map(N2kMessage::SystemTime)
                .unwrap_or(N2kMessage::Unknown(pgn, data.to_vec())),
            127250 => VesselHeading::from_bytes(data)
                .map(N2kMessage::VesselHeading)
                .unwrap_or(N2kMessage::Unknown(pgn, data.to_vec())),
            127251 => RateOfTurn::from_bytes(data)
                .map(N2kMessage::RateOfTurn)
                .unwrap_or(N2kMessage::Unknown(pgn, data.to_vec())),
            127257 => Attitude::from_bytes(data)
                .map(N2kMessage::Attitude)
                .unwrap_or(N2kMessage::Unknown(pgn, data.to_vec())),
            127488 => EngineRapidUpdate::from_bytes(data)
                .map(N2kMessage::EngineRapidUpdate)
                .unwrap_or(N2kMessage::Unknown(pgn, data.to_vec())),
            128259 => SpeedWaterReferenced::from_bytes(data)
                .map(N2kMessage::SpeedWaterReferenced)
                .unwrap_or(N2kMessage::Unknown(pgn, data.to_vec())),
            128267 => WaterDepth::from_bytes(data)
                .map(N2kMessage::WaterDepth)
                .unwrap_or(N2kMessage::Unknown(pgn, data.to_vec())),
            129025 => PositionRapidUpdate::from_bytes(data)
                .map(N2kMessage::PositionRapidUpdate)
                .unwrap_or(N2kMessage::Unknown(pgn, data.to_vec())),
            129026 => CogSogRapidUpdate::from_bytes(data)
                .map(N2kMessage::CogSogRapidUpdate)
                .unwrap_or(N2kMessage::Unknown(pgn, data.to_vec())),
            129029 => GnssPositionData::from_bytes(data)
                .map(N2kMessage::GnssPositionData)
                .unwrap_or(N2kMessage::Unknown(pgn, data.to_vec())),
            129033 => TimeDate::from_bytes(data)
                .map(N2kMessage::TimeDate)
                .unwrap_or(N2kMessage::Unknown(pgn, data.to_vec())),
            130306 => WindData::from_bytes(data)
                .map(N2kMessage::WindData)
                .unwrap_or(N2kMessage::Unknown(pgn, data.to_vec())),
            130312 => Temperature::from_bytes(data)
                .map(N2kMessage::Temperature)
                .unwrap_or(N2kMessage::Unknown(pgn, data.to_vec())),
            130313 => Humidity::from_bytes(data)
                .map(N2kMessage::Humidity)
                .unwrap_or(N2kMessage::Unknown(pgn, data.to_vec())),
            130314 => ActualPressure::from_bytes(data)
                .map(N2kMessage::ActualPressure)
                .unwrap_or(N2kMessage::Unknown(pgn, data.to_vec())),
            _ => N2kMessage::Unknown(pgn, data.to_vec()),
        }
    }
}

impl fmt::Display for N2kMessage {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            N2kMessage::SystemTime(msg) => write!(f, "{}", msg),
            N2kMessage::VesselHeading(msg) => write!(f, "{}", msg),
            N2kMessage::RateOfTurn(msg) => write!(f, "{}", msg),
            N2kMessage::Attitude(msg) => write!(f, "{}", msg),
            N2kMessage::EngineRapidUpdate(msg) => write!(f, "{}", msg),
            N2kMessage::SpeedWaterReferenced(msg) => write!(f, "{}", msg),
            N2kMessage::WaterDepth(msg) => write!(f, "{}", msg),
            N2kMessage::PositionRapidUpdate(msg) => write!(f, "{}", msg),
            N2kMessage::CogSogRapidUpdate(msg) => write!(f, "{}", msg),
            N2kMessage::GnssPositionData(msg) => write!(f, "{}", msg),
            N2kMessage::TimeDate(msg) => write!(f, "{}", msg),
            N2kMessage::WindData(msg) => write!(f, "{}", msg),
            N2kMessage::Temperature(msg) => write!(f, "{}", msg),
            N2kMessage::Humidity(msg) => write!(f, "{}", msg),
            N2kMessage::ActualPressure(msg) => write!(f, "{}", msg),
            N2kMessage::Unknown(_pgn, data) => {
                write!(f, "      Raw data: [{}]", format_data_bytes(data))
            }
        }
    }
}
