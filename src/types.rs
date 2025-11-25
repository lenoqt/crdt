use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug)]
pub enum CrdtError {
    Zenoh(zenoh::Error),
    Yrs(String),
    Json(serde_json::Error),
    Actor(String), // Kameo error types can be complex, simplifying for now
    Io(std::io::Error),
    #[allow(dead_code)]
    Other(String),
}

impl fmt::Display for CrdtError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CrdtError::Zenoh(e) => write!(f, "Zenoh error: {}", e),
            CrdtError::Yrs(e) => write!(f, "Yrs error: {}", e),
            CrdtError::Json(e) => write!(f, "Serialization error: {}", e),
            CrdtError::Actor(e) => write!(f, "Actor error: {}", e),
            CrdtError::Io(e) => write!(f, "IO error: {}", e),
            CrdtError::Other(e) => write!(f, "Other error: {}", e),
        }
    }
}

impl std::error::Error for CrdtError {}

impl From<zenoh::Error> for CrdtError {
    fn from(err: zenoh::Error) -> Self {
        CrdtError::Zenoh(err)
    }
}

impl From<serde_json::Error> for CrdtError {
    fn from(err: serde_json::Error) -> Self {
        CrdtError::Json(err)
    }
}

impl From<std::io::Error> for CrdtError {
    fn from(err: std::io::Error) -> Self {
        CrdtError::Io(err)
    }
}

#[derive(Serialize, Deserialize)]
pub enum PresenceMessage {
    Presence(u8),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash, Copy)]
pub struct RobotId(pub u8);

impl fmt::Display for RobotId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Default, Deserialize, Debug, Serialize, Clone, PartialEq)]
pub struct RobotPosition {
    pub id: u8,
    pub x: f32,
    pub y: f32,
}

impl fmt::Display for RobotPosition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "(id: {}, x: {:.1}, y: {:.1})", self.id, self.x, self.y)
    }
}

impl RobotPosition {
    pub fn new(id: u8, x: f32, y: f32) -> Self {
        RobotPosition { id, x, y }
    }
    pub fn add_x(&mut self, value: f32) {
        self.x += value;
    }
    pub fn add_y(&mut self, value: f32) {
        self.y += value;
    }
}
