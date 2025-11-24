use serde::{Deserialize, Serialize};
use std::fmt;

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
