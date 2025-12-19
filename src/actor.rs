use kameo::prelude::*;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::marker::PhantomData;
use std::sync::Arc;
use tokio::sync::oneshot;
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;
use yrs::{Doc, Map, Origin, ReadTxn, Subscription, Transact, Update, WriteTxn};

use crate::types::CrdtError as CRDTError;

// --- Messages ---

#[derive(Reply)]
pub struct GetRemoteRefsResponse(pub Arc<Vec<RemoteActorRef<DocumentRelay>>>);

#[derive(Clone, Serialize, Deserialize)]
pub struct UpdateRemoteRefs(pub Vec<RemoteActorRef<DocumentRelay>>);

#[derive(Clone, Serialize, Deserialize)]
pub struct GetRemotes;

#[derive(Clone, Serialize, Deserialize, Reply)]
pub struct DocumentUpdate(pub Vec<u8>);

#[derive(Clone, Serialize, Deserialize)]
pub struct GetData(pub String);

#[derive(Clone, Serialize, Deserialize)]
pub struct GetAllData;

#[derive(Clone, Serialize, Deserialize)]
pub struct SetData<T>(pub String, pub T);

#[derive(Debug, Reply)]
pub struct GetDataResponse<T: Send + 'static> {
    pub data: Option<T>,
}

#[derive(Debug, Reply)]
pub struct GetAllDataResponse<T: Send + 'static> {
    pub data: std::collections::HashMap<String, T>,
}

#[derive(Clone, Serialize, Deserialize, Reply)]
pub struct RelayInfo {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct GetRelayInfo;

#[derive(Clone, Serialize, Deserialize)]
pub struct SyncStep1(pub Vec<u8>);

#[derive(Clone, Serialize, Deserialize, Reply)]
pub struct SyncStep2(pub Vec<u8>);

// Internal message for handling sync locally
pub struct LocalSyncStep1(pub Vec<u8>, pub oneshot::Sender<SyncStep2>);

// --- Document Actor ---

pub struct Document<T> {
    document: Doc,
    _relay: ActorRef<DocumentRelay>,
    _subscription: Subscription,
    _marker: PhantomData<T>,
}

impl<T> Document<T>
where
    T: Send + Sync + Serialize + DeserializeOwned + 'static,
{
    pub(crate) fn new(
        document: Doc,
        subscription: Subscription,
        relay: ActorRef<DocumentRelay>,
    ) -> Self {
        Self {
            document,
            _relay: relay,
            _subscription: subscription,
            _marker: PhantomData,
        }
    }

    pub(crate) fn name() -> &'static str {
        let full = std::any::type_name::<T>();
        let name = full.rsplit("::").next().unwrap_or(full).to_lowercase();
        Box::leak(name.into_boxed_str())
    }

    pub(crate) fn set(&self, key: &str, value: &T) -> Result<(), CRDTError> {
        let mut txn = self.document.transact_mut_with(Self::name());
        let map = txn.get_or_insert_map(Self::name());
        let bytes =
            serde_json::to_vec(value).map_err(|e| CRDTError::Serialization(e.to_string()))?;
        map.insert(&mut txn, key, bytes);
        Ok(())
    }

    pub(crate) fn get(&self, key: &str) -> Result<Option<T>, CRDTError> {
        let txn = self.document.transact();
        if let Some(map) = txn.get_map(Self::name()) {
            if let Some(yrs::Out::Any(yrs::Any::Buffer(bytes))) = map.get(&txn, key) {
                let output = serde_json::from_slice::<T>(&bytes)
                    .map_err(|e| CRDTError::Serialization(e.to_string()))?;
                return Ok(Some(output));
            }
        }
        Ok(None)
    }

    pub(crate) fn get_all(&self) -> Result<std::collections::HashMap<String, T>, CRDTError> {
        let txn = self.document.transact();
        let mut results = std::collections::HashMap::new();
        if let Some(map) = txn.get_map(Self::name()) {
            for (key, val) in map.iter(&txn) {
                if let yrs::Out::Any(yrs::Any::Buffer(bytes)) = val {
                    if let Ok(parsed) = serde_json::from_slice::<T>(&bytes) {
                        results.insert(key.to_string(), parsed);
                    }
                }
            }
        }
        Ok(results)
    }

    pub(crate) fn apply_update(&self, update: &[u8], origin: Origin) {
        let origin_str = String::from_utf8_lossy(origin.as_ref()).to_string();
        let mut txn = self.document.transact_mut_with(origin);
        match Update::decode_v1(update) {
            Ok(update) => {
                if let Err(e) = txn.apply_update(update) {
                    println!("Failed to apply update from {}: {}", origin_str, e);
                }
            }
            Err(e) => {
                println!("Failed to decode update from {}: {}", origin_str, e);
            }
        }
    }
}

pub struct DocumentContext {
    pub id: String,
}

impl<T> Actor for Document<T>
where
    T: Send + Sync + Serialize + DeserializeOwned + 'static,
{
    type Args = DocumentContext;
    type Error = Infallible;

    async fn on_start(args: Self::Args, actor_ref: ActorRef<Self>) -> Result<Self, Self::Error> {
        let recipient_update = actor_ref.clone().recipient::<DocumentUpdate>();
        let recipient_sync = actor_ref.clone().recipient::<LocalSyncStep1>();
        let document = Doc::new();

        let relay = match DocumentRelay::initialize(
            args.id,
            Self::name(),
            recipient_update,
            recipient_sync,
        )
        .await
        {
            Ok(relay) => relay,
            Err(e) => {
                println!("Failed to initialize relay: {}", e);
                panic!("Cannot initialize Document without relay: {}", e);
            }
        };

        // Start periodic sync
        let relay_clone = relay.clone();
        let doc_clone = document.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
            loop {
                interval.tick().await;
                let sv = doc_clone.transact().state_vector().encode_v1();
                match relay_clone.ask(GetRemotes).await {
                    Ok(remotes) => {
                        for remote in remotes.0.iter() {
                            let msg = SyncStep1(sv.clone());
                            // Ask remote for diff
                            match remote.ask(&msg).await {
                                Ok(diff) => {
                                    // Apply diff
                                    if !diff.0.is_empty() {
                                        println!("Applied sync diff of size: {}", diff.0.len());
                                        let mut txn = doc_clone.transact_mut_with("sync");
                                        if let Ok(update) = Update::decode_v1(&diff.0) {
                                            if let Err(e) = txn.apply_update(update) {
                                                println!("Failed to apply sync update: {}", e);
                                            }
                                        }
                                    }
                                }
                                Err(e) => println!("Sync request failed: {}", e),
                            }
                        }
                    }
                    Err(e) => println!("Sync loop failed to get remotes: {}", e),
                }
            }
        });

        let relay_subscription = relay.clone();
        let subscription = document
            .observe_update_v1(move |txn, _update| {
                let is_local = txn
                    .origin()
                    .is_some_and(|origin| String::from_utf8_lossy(origin.as_ref()) == Self::name());
                if !is_local {
                    return;
                }

                let previous_sv = txn.before_state();
                let update_data = txn.encode_state_as_update_v1(&previous_sv);
                println!("Generated update of size: {}", update_data.len());
                let relay = relay_subscription.clone();

                tokio::spawn(async move {
                    let remotes = match relay.ask(GetRemotes).await {
                        Ok(r) => r,
                        Err(e) => {
                            println!("Failed to fetch remotes: {}", e);
                            return;
                        }
                    };
                    for remote in remotes.0.iter() {
                        let update = update_data.clone();
                        let remote = remote.clone();
                        tokio::spawn(async move {
                            let msg = DocumentUpdate(update);
                            if let Err(e) = remote.tell(&msg).send() {
                                println!("Failed to send update to remote: {}", e);
                            }
                        });
                    }
                });
            })
            .map_err(|e| {
                println!("Subscription initialization failed: {}", e);
                panic!("Cannot create Document without subscription: {}", e);
            })
            .expect("subscription error handled above");

        Ok(Self::new(document, subscription, relay))
    }
}

impl<T> Message<LocalSyncStep1> for Document<T>
where
    T: Send + Sync + Serialize + DeserializeOwned + 'static,
{
    type Reply = (); // We reply via oneshot

    async fn handle(&mut self, msg: LocalSyncStep1, _: &mut Context<Self, Self::Reply>) {
        match yrs::StateVector::decode_v1(&msg.0) {
            Ok(sv) => {
                let txn = self.document.transact();
                let diff = txn.encode_state_as_update_v1(&sv);
                let _ = msg.1.send(SyncStep2(diff));
            }
            Err(e) => {
                println!("Failed to decode SV in sync request: {}", e);
                let _ = msg.1.send(SyncStep2(vec![]));
            }
        }
    }
}

impl<T> Message<DocumentUpdate> for Document<T>
where
    T: Send + Sync + Serialize + DeserializeOwned + 'static,
{
    type Reply = ();

    async fn handle(&mut self, msg: DocumentUpdate, _: &mut Context<Self, Self::Reply>) {
        println!("Received DocumentUpdate of size: {}", msg.0.len());
        self.apply_update(&msg.0, "remote".into());
    }
}

impl<T> Message<GetData> for Document<T>
where
    T: Send + Sync + Serialize + DeserializeOwned + 'static,
{
    type Reply = GetDataResponse<T>;

    async fn handle(&mut self, msg: GetData, _: &mut Context<Self, Self::Reply>) -> Self::Reply {
        match self.get(&msg.0) {
            Ok(data) => GetDataResponse { data },
            Err(e) => {
                println!("GetData failed: {}", e);
                GetDataResponse { data: None }
            }
        }
    }
}

impl<T> Message<GetAllData> for Document<T>
where
    T: Send + Sync + Serialize + DeserializeOwned + 'static,
{
    type Reply = GetAllDataResponse<T>;

    async fn handle(&mut self, _: GetAllData, _: &mut Context<Self, Self::Reply>) -> Self::Reply {
        match self.get_all() {
            Ok(data) => GetAllDataResponse { data },
            Err(e) => {
                println!("GetAllData failed: {}", e);
                GetAllDataResponse {
                    data: std::collections::HashMap::new(),
                }
            }
        }
    }
}

impl<T> Message<SetData<T>> for Document<T>
where
    T: Send + Sync + Serialize + DeserializeOwned + 'static,
{
    type Reply = ();

    async fn handle(&mut self, msg: SetData<T>, _: &mut Context<Self, Self::Reply>) {
        if let Err(e) = self.set(&msg.0, &msg.1) {
            println!("SetData failed: {}", e);
        }
    }
}

// --- Document Relay ---

#[derive(Actor, RemoteActor, Clone)]
pub struct DocumentRelay {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) remotes: Arc<Vec<RemoteActorRef<DocumentRelay>>>,
    pub(crate) local_update: Recipient<DocumentUpdate>,
    pub(crate) local_sync: Recipient<LocalSyncStep1>,
}

impl DocumentRelay {
    fn new(
        id: String,
        name: String,
        local_update: Recipient<DocumentUpdate>,
        local_sync: Recipient<LocalSyncStep1>,
    ) -> Self {
        Self {
            id,
            name,
            remotes: Arc::new(Vec::new()),
            local_update,
            local_sync,
        }
    }

    async fn initialize(
        id: String,
        name: &str,
        local_update: Recipient<DocumentUpdate>,
        local_sync: Recipient<LocalSyncStep1>,
    ) -> Result<ActorRef<Self>, CRDTError> {
        let actor = DocumentRelay::new(id, name.to_string(), local_update, local_sync);
        let actor_ref = DocumentRelay::spawn(actor);

        actor_ref.register(name).await.map_err(|e| {
            CRDTError::Registration(format!("Failed to register relay '{name}': {e}"))
        })?;

        let peer_id = actor_ref
            .id()
            .peer_id()
            .map(|p| p.to_string())
            .unwrap_or_default();

        let remotes = Self::get_remotes(&peer_id, name).await?;
        actor_ref
            .tell(UpdateRemoteRefs(remotes))
            .send()
            .await
            .map_err(|e| CRDTError::ActorSend(format!("Failed to update remotes: {e}")))?;

        Self::start_remotes_refresh(actor_ref.clone());
        Ok(actor_ref)
    }

    async fn get_remotes(
        peer_id: &str,
        name: &str,
    ) -> Result<Vec<RemoteActorRef<Self>>, CRDTError> {
        use futures_util::stream::TryStreamExt;

        RemoteActorRef::<Self>::lookup_all(name)
            .try_collect::<Vec<_>>()
            .await
            .map_err(|e| CRDTError::RemoteLookup(e.to_string()))
            .map(|refs| {
                refs.into_iter()
                    .filter(|r| {
                        r.id()
                            .peer_id()
                            .is_some_and(|peer| peer.to_string() != peer_id)
                    })
                    .collect()
            })
    }

    fn start_remotes_refresh(actor_ref: ActorRef<Self>) {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));
            loop {
                interval.tick().await;

                if !actor_ref.is_alive() {
                    println!("Relay actor stopped, ending refresh loop");
                    break;
                }

                let name = match actor_ref.ask(GetRelayInfo).await {
                    Ok(info) => info.name,
                    Err(e) => {
                        println!("Failed to get relay info: {}", e);
                        break;
                    }
                };

                let peer_id = actor_ref
                    .id()
                    .peer_id()
                    .map(|p| p.to_string())
                    .unwrap_or_default();

                match Self::get_remotes(&peer_id, &name).await {
                    Ok(remotes) => {
                        if !remotes.is_empty() {
                            println!("Found {} remotes for {}", remotes.len(), name);
                        }
                        if let Err(e) = actor_ref.tell(UpdateRemoteRefs(remotes)).send().await {
                            println!("Failed to update remotes: {}", e);
                            break;
                        }
                    }
                    Err(e) => println!("Remote sync failed: {}", e),
                }
            }
        });
    }
}

#[kameo::remote_message("sync_step_1")]
impl Message<SyncStep1> for DocumentRelay {
    type Reply = SyncStep2;

    async fn handle(&mut self, msg: SyncStep1, _: &mut Context<Self, Self::Reply>) -> Self::Reply {
        let (tx, rx) = oneshot::channel();
        let local_msg = LocalSyncStep1(msg.0, tx);

        if let Err(e) = self.local_sync.tell(local_msg).send().await {
            println!("Failed to forward SyncStep1 to local document: {}", e);
            return SyncStep2(vec![]);
        }

        match rx.await {
            Ok(resp) => resp,
            Err(e) => {
                println!("Failed to receive sync response from local document: {}", e);
                SyncStep2(vec![])
            }
        }
    }
}

#[kameo::remote_message("document_update")]
impl Message<DocumentUpdate> for DocumentRelay {
    type Reply = ();

    async fn handle(&mut self, msg: DocumentUpdate, _: &mut Context<Self, Self::Reply>) {
        let _ = self.local_update.tell(msg).send().await;
    }
}

impl Message<UpdateRemoteRefs> for DocumentRelay {
    type Reply = ();

    async fn handle(&mut self, msg: UpdateRemoteRefs, _: &mut Context<Self, Self::Reply>) {
        self.remotes = Arc::new(msg.0);
    }
}

impl Message<GetRemotes> for DocumentRelay {
    type Reply = GetRemoteRefsResponse;

    async fn handle(&mut self, _: GetRemotes, _: &mut Context<Self, Self::Reply>) -> Self::Reply {
        GetRemoteRefsResponse(Arc::clone(&self.remotes))
    }
}

impl Message<GetRelayInfo> for DocumentRelay {
    type Reply = RelayInfo;

    async fn handle(
        &mut self,
        _: GetRelayInfo,
        _: &mut kameo::prelude::Context<Self, Self::Reply>,
    ) -> Self::Reply {
        RelayInfo {
            id: self.id.clone(),
            name: self.name.clone(),
        }
    }
}
