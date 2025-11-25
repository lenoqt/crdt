use std::any::Any;
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use zenoh::Session;

use crate::actor::{Document, DocumentContext, DocumentTag};
use crate::types::{CrdtError, NetworkStats, PresenceMessage, RobotId, RobotPosition};
use kameo::actor::{ActorRef, Spawn};

const DISCOVERY_TOPIC: &str = "crdt/discovery";

#[derive(Debug)]
pub struct RegisteredActorRef {
    pub id: RobotId,
    pub actor_ref: Box<dyn Any + Send + Sync>,
}

#[derive(Clone, Debug)]
pub struct DocumentRegistry {
    doc_actor_refs: Arc<RwLock<HashMap<Cow<'static, str>, RegisteredActorRef>>>,
    session: Session,
    stats: NetworkStats, 
    owned_id: RobotId,
}

impl DocumentRegistry {
    pub fn builder() -> DocumentRegistryBuilder {
        DocumentRegistryBuilder::default()
    }
    
    pub fn get_stats(&self) -> NetworkStats {
        self.stats.clone()
    }

    pub async fn add_remote(
        &self,
        id: RobotId,
    ) -> Result<Option<ActorRef<Document<RobotPosition>>>, CrdtError> {
        // Check with read lock first
        {
            let actors = self.doc_actor_refs.read().await;
            let key = id.to_string();
            if actors.contains_key(key.as_str()) {
                return Ok(None);
            }
        } // Read lock dropped here

        // Acquire write lock
        let mut actors = self.doc_actor_refs.write().await;
        let key = Cow::Owned(id.to_string());

        // Double check in case another task added it while we were waiting for write lock
        if actors.contains_key(&key) {
            return Ok(None);
        }

        println!("Discovered remote peer: {}", id);
        let remote_ctx = DocumentContext {
            session: self.session.clone(),
            robot_id: id,
            origin: DocumentTag::Remote(yrs::Origin::from(id.to_string())),
            initial_state: Some(RobotPosition::new(id.0, 0.0, 0.0)),
            stats: self.stats.clone(),
        };
        let remote_actor = Document::<RobotPosition>::spawn(remote_ctx);

        actors.insert(
            key,
            RegisteredActorRef {
                id,
                actor_ref: Box::new(remote_actor.clone()),
            },
        );

        Ok(Some(remote_actor))
    }

    pub async fn get_actor(&self, id: RobotId) -> Option<ActorRef<Document<RobotPosition>>> {
        let actors = self.doc_actor_refs.read().await;
        let key = id.to_string();
        actors.get(key.as_str()).and_then(|r| {
            r.actor_ref
                .downcast_ref::<ActorRef<Document<RobotPosition>>>()
                .cloned()
        })
    }

    pub async fn get_owned(&self) -> ActorRef<Document<RobotPosition>> {
        self.get_actor(self.owned_id)
            .await
            .expect("Owned actor must exist")
    }

    pub async fn get_all_remotes(&self) -> Vec<(RobotId, ActorRef<Document<RobotPosition>>)> {
        let actors = self.doc_actor_refs.read().await;
        actors
            .values()
            .filter_map(|r| {
                if r.id == self.owned_id {
                    None
                } else {
                    r.actor_ref
                        .downcast_ref::<ActorRef<Document<RobotPosition>>>()
                        .map(|actor| (r.id, actor.clone()))
                }
            })
            .collect()
    }
}

#[derive(Default)]
pub struct DocumentRegistryBuilder {
    session: Option<Session>,
    id: Option<RobotId>,
}

impl DocumentRegistryBuilder {
    pub fn session(mut self, session: Session) -> Self {
        self.session = Some(session);
        self
    }

    pub fn id(mut self, id: RobotId) -> Self {
        self.id = Some(id);
        self
    }

    pub async fn build(self) -> Result<DocumentRegistry, CrdtError> {
        let session = self
            .session
            .ok_or_else(|| CrdtError::Other("Session is required".to_string()))?;
        let owned_id = self
            .id
            .ok_or_else(|| CrdtError::Other("RobotId is required".to_string()))?;
        let stats = NetworkStats::new();

        let registry = DocumentRegistry {
            doc_actor_refs: Arc::new(RwLock::new(HashMap::new())),
            session: session.clone(),
            owned_id,
            stats: stats.clone(),
        };

        // Create owned actor
        let owned_ctx = DocumentContext {
            session: session.clone(),
            robot_id: owned_id,
            origin: DocumentTag::Owned(yrs::Origin::from(owned_id.to_string())),
            initial_state: Some(RobotPosition::new(owned_id.0, 0.0, 0.0)),
            stats: stats.clone(),
        };
        let owned_actor = Document::<RobotPosition>::spawn(owned_ctx);

        // Add owned actor to registry
        {
            let mut actors = registry.doc_actor_refs.write().await;
            actors.insert(
                Cow::Owned(owned_id.to_string()),
                RegisteredActorRef {
                    id: owned_id,
                    actor_ref: Box::new(owned_actor),
                },
            );
        }

        // Spawn discovery publisher
        let session_pub = session.clone();
        tokio::spawn(async move {
            loop {
                let msg = PresenceMessage::Presence(owned_id.0);
                if let Ok(payload) = serde_json::to_string(&msg) {
                    if let Err(e) = session_pub.put(DISCOVERY_TOPIC, payload).await {
                        eprintln!("Failed to publish presence: {}", e);
                    }
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        });

        // Spawn discovery subscriber
        let registry_clone = registry.clone();
        let sub = session
            .declare_subscriber(DISCOVERY_TOPIC)
            .await
            .map_err(CrdtError::Zenoh)?;

        tokio::spawn(async move {
            while let Ok(sample) = sub.recv_async().await {
                if let Ok(msg_str) = std::str::from_utf8(&sample.payload().to_bytes().to_vec()) {
                    if let Ok(PresenceMessage::Presence(id)) = serde_json::from_str(msg_str) {
                        let remote_id = RobotId(id);
                        if remote_id != owned_id {
                            if let Err(e) = registry_clone.add_remote(remote_id).await {
                                eprintln!("Failed to add remote peer: {}", e);
                            }
                        }
                    }
                }
            }
        });

        Ok(registry)
    }
}
