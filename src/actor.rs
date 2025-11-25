use kameo::prelude::Context;
use kameo::{Actor, Reply};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::marker::PhantomData;
use std::sync::Arc;
use std::sync::Mutex;
use tokio::task::spawn;
use yrs::{
    Array, Doc, Map, ReadTxn, StateVector, Subscription, Transact, Update, WriteTxn,
    updates::decoder::Decode,
};
use zenoh::Session;

use crate::types::{CrdtError, NetworkStats, RobotId};

#[derive(Clone)]
pub enum DocumentTag {
    Owned(yrs::Origin),
    Remote(yrs::Origin),
}

impl DocumentTag {
    pub fn as_origin(&self) -> &yrs::Origin {
        match self {
            DocumentTag::Owned(origin) => origin,
            DocumentTag::Remote(origin) => origin,
        }
    }
}

pub struct Document<T: Serialize + DeserializeOwned + Default + Send + Clone + 'static> {
    doc: Doc,
    tag: DocumentTag,
    _subscription: Subscription,
    fallback_state: Option<T>,
    _stats: NetworkStats,
    _marker: PhantomData<T>,
    _last_sync_state: Arc<Mutex<StateVector>>,
}

impl<T> Document<T>
where
    T: Serialize + DeserializeOwned + Default + Send + Clone,
{
    fn set(&self, value: &T, origin: &yrs::Origin) -> Result<(), CrdtError> {
        let bytes = serde_json::to_vec(value)?;
        let mut txn = self.doc.transact_mut_with(origin.clone());
        let map = txn.get_or_insert_map("state");
        map.insert(&mut txn, "value", bytes);  // Single operation - replaces old value
        Ok(())
    }



    fn get(&self) -> T {
        let txn = self.doc.transact();
        if let Some(map) = txn.get_map("state") {
            if let Some(yrs::Out::Any(yrs::Any::Buffer(bytes))) = map.get(&txn, "value") {
                return serde_json::from_slice(&bytes).unwrap_or_default();
            }
        }
        self.fallback_state.clone().unwrap_or_default()
    }

    fn apply_update(&self, update_bytes: &[u8]) -> Result<(), CrdtError> {
        let mut txn = self.doc.transact_mut();
        let update = Update::decode_v1(update_bytes).map_err(|e| CrdtError::Yrs(e.to_string()))?;
        txn.apply_update(update)
            .map_err(|e| CrdtError::Yrs(e.to_string()))?;
        Ok(())
    }
}

pub enum DocumentMessage<T> {
    Set(T),
    Get,
    ApplyUpdate(Vec<u8>),
}

#[derive(Reply, Debug)]
#[allow(dead_code)]
pub enum DocumentReply<T: Send + 'static> {
    Set(Result<(), CrdtError>),
    Get(T),
    ApplyUpdate(Result<(), CrdtError>),
}

impl<T> DocumentReply<T>
where
    T: Default + Serialize + DeserializeOwned + Send + 'static,
{
    pub fn inner(self) -> T {
        match self {
            DocumentReply::Get(value) => value,
            _ => panic!("Unexpected reply"),
        }
    }
}

use kameo::prelude::Message;

impl<T> Message<DocumentMessage<T>> for Document<T>
where
    T: Default + Serialize + DeserializeOwned + Send + Clone + 'static,
{
    type Reply = DocumentReply<T>;

    async fn handle(
        &mut self,
        msg: DocumentMessage<T>,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        match msg {
            DocumentMessage::Set(value) => {
                let res = self.set(&value, self.tag.as_origin());
                DocumentReply::Set(res)
            }
            DocumentMessage::Get => DocumentReply::Get(self.get()),
            DocumentMessage::ApplyUpdate(update) => {
                let res = self.apply_update(&update);
                if let Err(e) = &res {
                    eprintln!("Failed to apply update: {}", e);
                }
                DocumentReply::ApplyUpdate(res)
            }
        }
    }
}

pub struct DocumentContext<T> {
    pub session: Session,
    pub robot_id: RobotId,
    pub origin: DocumentTag,
    pub stats: NetworkStats, 
    pub initial_state: Option<T>,
}

impl<T> Actor for Document<T>
where
    T: Serialize + DeserializeOwned + Default + Send + Clone + 'static,
{
    type Args = DocumentContext<T>;
    type Error = kameo::error::Infallible;

    async fn on_start(
        args: Self::Args,
        actor_ref: kameo::actor::ActorRef<Self>,
    ) -> Result<Self, Self::Error> {
        let doc = Doc::new();
        let topic = format!("{}/position/sync", args.robot_id);
        let topic_for_sub = topic.clone();
        let session_clone = args.session.clone();
        let stats_clone = args.stats.clone();
        let last_sync = Arc::new(Mutex::new(StateVector::default()));
        let last_sync_clone = last_sync.clone();    

        let is_owned = matches!(args.origin, DocumentTag::Owned(_));
        let subscription = doc
            .observe_update_v1(move |txn, _update_event| {
                if is_owned && txn.origin().is_some() {
                    let last_sv = last_sync_clone.lock().unwrap().clone();
                    let update = txn.encode_state_as_update_v1(&last_sv);
                    *last_sync_clone.lock().unwrap() = txn.state_vector();     
                    stats_clone.record_sent(update.len());
                    let topic = topic.clone();
                    let session = session_clone.clone();
                    spawn(async move {
                        if let Err(e) = session.put(&topic, update).await {
                            eprintln!("Failed to publish update: {}", e);
                        }
                    });
                }
            })
            .expect("Failed subscription");

        if let DocumentTag::Remote(_) = args.origin {
            let session_sub = args.session.clone();
            let stats_sub = args.stats.clone();
            spawn(async move {
                println!("Subscribing to {}", topic_for_sub);
                let subscriber = session_sub
                    .declare_subscriber(&topic_for_sub)
                    .await
                    .unwrap();
                while let Ok(sample) = subscriber.recv_async().await {
                    let payload = sample.payload().to_bytes().to_vec();
                    stats_sub.record_received(payload.len());  // TRACK RECEIVED
                    let _ = actor_ref.tell(DocumentMessage::ApplyUpdate(payload)).await;
                }
            });
        }

        let actor = Self {
            doc,
            tag: args.origin,
            _subscription: subscription,
            fallback_state: args.initial_state,
            _stats: args.stats,
            _last_sync_state: last_sync,
            _marker: PhantomData,
        };

        Ok(actor)
    }
}
