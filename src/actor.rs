use kameo::prelude::Context;
use kameo::{Actor, Reply};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::marker::PhantomData;
use tokio::task::spawn;
use yrs::{
    Array, Doc, ReadTxn, StateVector, Subscription, Transact, Update, WriteTxn,
    updates::decoder::Decode,
};
use zenoh::Session;

use crate::types::RobotId;

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
    _marker: PhantomData<T>,
}

impl<T> Document<T>
where
    T: Serialize + DeserializeOwned + Default + Send + Clone,
{
    fn set(&self, value: &T, origin: &yrs::Origin) {
        let bytes = serde_json::to_vec(value).unwrap();
        let mut txn = self.doc.transact_mut_with(origin.clone());
        let array = txn.get_or_insert_array("state");

        let len = array.len(&txn);
        if len > 0 {
            array.remove_range(&mut txn, 0, len);
        }
        array.push_back(&mut txn, bytes);
    }

    fn get(&self) -> T {
        let txn = self.doc.transact();
        if let Some(array) = txn.get_array("state") {
            if array.len(&txn) > 0 {
                if let Some(value) = array.get(&txn, 0) {
                    if let yrs::Out::Any(yrs::Any::Buffer(bytes)) = value {
                        return serde_json::from_slice(&bytes).unwrap_or_default();
                    }
                }
            }
        }
        self.fallback_state.clone().unwrap_or_default()
    }

    fn apply_update(&self, update_bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        let mut txn = self.doc.transact_mut();
        let update = Update::decode_v1(update_bytes)?;
        txn.apply_update(update)?;
        Ok(())
    }
}

pub enum DocumentMessage<T> {
    Set(T),
    Get,
    ApplyUpdate(Vec<u8>),
}

#[derive(Reply, Debug)]
pub enum DocumentReply<T: Send + 'static> {
    Set(()),
    Get(T),
    ApplyUpdate,
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
                self.set(&value, self.tag.as_origin());
                DocumentReply::Set(())
            }
            DocumentMessage::Get => DocumentReply::Get(self.get()),
            DocumentMessage::ApplyUpdate(update) => {
                if let Err(e) = self.apply_update(&update) {
                    eprintln!("Failed to apply update: {}", e);
                }
                DocumentReply::ApplyUpdate
            }
        }
    }
}

pub struct DocumentContext<T> {
    pub session: Session,
    pub robot_id: RobotId,
    pub origin: DocumentTag,
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

        let is_owned = matches!(args.origin, DocumentTag::Owned(_));
        let subscription = doc
            .observe_update_v1(move |txn, _update_event| {
                if is_owned && txn.origin().is_some() {
                    let update = txn.encode_state_as_update_v1(&StateVector::default());
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
            spawn(async move {
                println!("Subscribing to {}", topic_for_sub);
                let subscriber = session_sub
                    .declare_subscriber(&topic_for_sub)
                    .await
                    .unwrap();
                while let Ok(sample) = subscriber.recv_async().await {
                    let payload = sample.payload().to_bytes().to_vec();
                    let _ = actor_ref.tell(DocumentMessage::ApplyUpdate(payload)).await;
                }
            });
        }

        let actor = Self {
            doc,
            tag: args.origin,
            _subscription: subscription,
            fallback_state: args.initial_state,
            _marker: PhantomData,
        };

        Ok(actor)
    }
}
