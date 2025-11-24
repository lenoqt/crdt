use std::any::Any;
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use zenoh::Session;

use crate::actor::{Document, DocumentContext, DocumentTag};
use crate::types::{RobotId, RobotPosition};
use kameo::actor::{ActorRef, Spawn};

#[derive(Debug)]
pub struct RegisteredActorRef {
    pub id: RobotId,
    pub actor_ref: Box<dyn Any + Send>,
}

#[derive(Debug, Default)]
pub struct DocumentRegistry {
    doc_actor_refs: Arc<Mutex<HashMap<Cow<'static, str>, RegisteredActorRef>>>,
}

impl DocumentRegistry {
    pub fn new() -> Self {
        Self {
            doc_actor_refs: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn add_remote(
        &self,
        id: RobotId,
        session: Session,
    ) -> Option<ActorRef<Document<RobotPosition>>> {
        let mut actors = self.doc_actor_refs.lock().await;
        let key = Cow::Owned(id.to_string());

        if actors.contains_key(&key) {
            return None;
        }

        println!("Discovered remote peer: {}", id);
        let remote_ctx = DocumentContext {
            session: session.clone(),
            robot_id: id,
            origin: DocumentTag::Remote(yrs::Origin::from(id.to_string())),
            initial_state: Some(RobotPosition::new(id.0, 0.0, 0.0)),
        };
        let remote_actor = Document::<RobotPosition>::spawn(remote_ctx);

        actors.insert(
            key,
            RegisteredActorRef {
                id,
                actor_ref: Box::new(remote_actor.clone()),
            },
        );

        Some(remote_actor)
    }

    #[allow(dead_code)]
    pub async fn get_remote(&self, id: RobotId) -> Option<ActorRef<Document<RobotPosition>>> {
        let actors = self.doc_actor_refs.lock().await;
        let key = id.to_string();
        actors.get(key.as_str()).and_then(|r| {
            r.actor_ref
                .downcast_ref::<ActorRef<Document<RobotPosition>>>()
                .cloned()
        })
    }

    pub async fn get_all_remotes(&self) -> Vec<(RobotId, ActorRef<Document<RobotPosition>>)> {
        let actors = self.doc_actor_refs.lock().await;
        actors
            .values()
            .filter_map(|r| {
                r.actor_ref
                    .downcast_ref::<ActorRef<Document<RobotPosition>>>()
                    .map(|actor| (r.id, actor.clone()))
            })
            .collect()
    }
}
