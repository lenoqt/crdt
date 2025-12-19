use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Serialize, Deserialize)]
pub enum CrdtError {
    Yrs(String),
    Json(String),
    Actor(String),
    Io(String),
    Serialization(String),
    Registration(String),
    ActorSend(String),
    RemoteLookup(String),
    #[allow(dead_code)]
    Other(String),
}

impl fmt::Display for CrdtError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CrdtError::Yrs(e) => write!(f, "Yrs error: {}", e),
            CrdtError::Json(e) => write!(f, "Serialization error: {}", e),
            CrdtError::Actor(e) => write!(f, "Actor error: {}", e),
            CrdtError::Io(e) => write!(f, "IO error: {}", e),
            CrdtError::Serialization(e) => write!(f, "Serialization error: {}", e),
            CrdtError::Registration(e) => write!(f, "Registration error: {}", e),
            CrdtError::ActorSend(e) => write!(f, "Actor send error: {}", e),
            CrdtError::RemoteLookup(e) => write!(f, "Remote lookup error: {}", e),
            CrdtError::Other(e) => write!(f, "Other error: {}", e),
        }
    }
}

impl std::error::Error for CrdtError {}

impl From<serde_json::Error> for CrdtError {
    fn from(err: serde_json::Error) -> Self {
        CrdtError::Serialization(err.to_string())
    }
}

impl From<std::io::Error> for CrdtError {
    fn from(err: std::io::Error) -> Self {
        CrdtError::Io(err.to_string())
    }
}

impl<M, E> From<kameo::error::SendError<M, E>> for CrdtError
where
    E: std::fmt::Debug,
{
    fn from(err: kameo::error::SendError<M, E>) -> Self {
        CrdtError::ActorSend(format!("{:?}", err))
    }
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

#[derive(Default, Deserialize, Debug, Serialize, Clone, PartialEq)]
pub struct ChatMessage {
    pub sender_id: u8,
    pub content: String,
    pub timestamp: u64,
}

impl fmt::Display for ChatMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{}] User {}: {}",
            self.timestamp, self.sender_id, self.content
        )
    }
}

pub trait CrdtModel:
    serde::Serialize
    + serde::de::DeserializeOwned
    + Clone
    + Send
    + Sync
    + 'static
    + Default
    + fmt::Display
{
    const ACTOR_ID: &'static str;
    const APPLY_ID: &'static str;
    const SYNC_ID: &'static str;
}

impl CrdtModel for RobotPosition {
    const ACTOR_ID: &'static str = "crdt_relay_v4";
    const APPLY_ID: &'static str = "crdt_apply_v4";
    const SYNC_ID: &'static str = "crdt_sync_v4";
}

impl CrdtModel for ChatMessage {
    const ACTOR_ID: &'static str = "chat_relay_v1";
    const APPLY_ID: &'static str = "chat_apply_v1";
    const SYNC_ID: &'static str = "chat_sync_v1";
}
