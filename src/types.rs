use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;


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


#[derive(Clone, Debug)]
pub struct NetworkStats {
    bytes_sent: Arc<AtomicU64>,
    bytes_received: Arc<AtomicU64>,
    messages_sent: Arc<AtomicU64>,
    messages_received: Arc<AtomicU64>,
}

impl NetworkStats {
    pub fn new() -> Self {
        Self {
            bytes_sent: Arc::new(AtomicU64::new(0)),
            bytes_received: Arc::new(AtomicU64::new(0)),
            messages_sent: Arc::new(AtomicU64::new(0)),
            messages_received: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn record_sent(&self, bytes: usize) {
        self.bytes_sent.fetch_add(bytes as u64, Ordering::Relaxed);
        self.messages_sent.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_received(&self, bytes: usize) {
        self.bytes_received.fetch_add(bytes as u64, Ordering::Relaxed);
        self.messages_received.fetch_add(1, Ordering::Relaxed);
    }

    pub fn get_stats(&self) -> (u64, u64, u64, u64) {
        (
            self.bytes_sent.load(Ordering::Relaxed),
            self.bytes_received.load(Ordering::Relaxed),
            self.messages_sent.load(Ordering::Relaxed),
            self.messages_received.load(Ordering::Relaxed),
        )
    }
}