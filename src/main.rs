use clap::Parser;
use crossterm::event::KeyCode;
use kameo::actor::Spawn;
use kameo::prelude::{Context, Message};
use kameo::{Actor, Reply};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::io::stdout;
use std::marker::PhantomData;

use tokio::task::spawn;
use yrs::updates::encoder::Encode;
use yrs::{Array, Subscription};
use yrs::{Doc, ReadTxn, StateVector, Transact, Update, WriteTxn, updates::decoder::Decode};

use zenoh::Session;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Copy)]
struct RobotId(u8);

impl std::fmt::Display for RobotId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Clone)]
enum DocumentTag {
    Owned(yrs::Origin),
    Remote(yrs::Origin),
}

impl DocumentTag {
    fn as_origin(&self) -> &yrs::Origin {
        match self {
            DocumentTag::Owned(origin) => origin,
            DocumentTag::Remote(origin) => origin,
        }
    }
}

struct Document<T: Serialize + DeserializeOwned + Default + Send + 'static> {
    doc: Doc,
    tag: DocumentTag,
    session: Session,
    _subscription: Subscription,
    _marker: PhantomData<T>,
}

impl<T> Document<T>
where
    T: Serialize + DeserializeOwned + Default + Send,
{
    fn set(&self, value: &T) {
        let bytes = serde_json::to_vec(value).unwrap();
        let mut txn = self.doc.transact_mut_with(self.tag.as_origin().clone());
        let array = txn.get_or_insert_array("state");

        // Clear and insert
        let len = array.len(&txn);
        if len > 0 {
            array.remove_range(&mut txn, 0, len);
        }
        array.push_back(&mut txn, bytes);
    }

    fn get(&self) -> T {
        let txn = self.doc.transact();
        if let Some(array) = txn.get_array("state")
            && let Some(value) = array.get(&txn, 0)
            && let yrs::Out::Any(yrs::Any::Buffer(bytes)) = value
        {
            return serde_json::from_slice(&bytes).unwrap_or_default();
        }
        T::default()
    }

    /// Apply remote CRDT update
    fn apply_update(&self, update_bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        let mut txn = self.doc.transact_mut_with("remote");
        let update = Update::decode_v1(update_bytes)?;
        let _ = txn.apply_update(update);
        Ok(())
    }

    fn state_vector(&self) -> Vec<u8> {
        self.doc.transact().state_vector().encode_v1()
    }

    fn encode_diff(&self, remote_state_vector: &[u8]) -> Vec<u8> {
        let remote_sv = StateVector::decode_v1(remote_state_vector).unwrap();
        self.doc.transact().encode_state_as_update_v1(&remote_sv)
    }
}

enum DocumentMessage<T> {
    Set(T),
    Get,
    ApplyUpdate(Vec<u8>),
}

#[derive(Reply, Debug)]
enum DocumentReply<T: Send + 'static> {
    Set(()),
    Get(T),
    ApplyUpdate,
}

impl<T> DocumentReply<T>
where
    T: Default + Serialize + DeserializeOwned + Send + 'static,
{
    fn inner(self) -> T {
        match self {
            DocumentReply::Set(_) => panic!("Unexpected update reply"),
            DocumentReply::Get(value) => value,
            DocumentReply::ApplyUpdate => panic!("Unexpected apply update reply"),
        }
    }
}

impl<T> Message<DocumentMessage<T>> for Document<T>
where
    T: Default + Serialize + DeserializeOwned + Send + 'static,
{
    type Reply = DocumentReply<T>;

    async fn handle(
        &mut self,
        msg: DocumentMessage<T>,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        match msg {
            DocumentMessage::Set(value) => {
                self.set(&value);
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

struct DocumentContext {
    session: zenoh::Session,
    robot_id: RobotId,
    origin: DocumentTag,
}

impl DocumentContext {
    fn new(session: zenoh::Session, robot_id: RobotId, origin: DocumentTag) -> Self {
        Self {
            session,
            robot_id,
            origin,
        }
    }
}

impl<T> Actor for Document<T>
where
    T: Serialize + DeserializeOwned + Default + Send + 'static,
{
    type Args = DocumentContext;
    type Error = kameo::error::Infallible;

    async fn on_start(
        args: Self::Args,

        actor_ref: kameo::actor::ActorRef<Self>,
    ) -> Result<Self, Self::Error> {
        let doc = Doc::new();

        let topic = format!("{}/position/sync", args.robot_id);

        let topic_for_sub = topic.clone();

        let session_clone = args.session.clone();

        let origin_clone = args.origin.clone();

        let subscription = doc
            .observe_update_v1(move |txn, update_event| {
                if let DocumentTag::Remote(_) = origin_clone {
                    return;
                }

                if let Some(origin) = txn.origin() {
                    if origin != origin_clone.as_origin() {
                        return;
                    }
                } else {
                    return;
                }

                let update = update_event.update.to_vec();

                let topic = topic.clone();

                let session = session_clone.clone();

                println!(
                    "Updated document on topic {}, publishing update size: {}",
                    topic,
                    update.len()
                );

                spawn(async move {
                    if let Err(e) = session.put(&topic, update).await {
                        eprintln!("Failed to publish update: {}", e);
                    }
                });
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

                    println!(
                        "Received update on topic {} size: {}",
                        sample.key_expr(),
                        payload.len()
                    );

                    let _ = actor_ref.tell(DocumentMessage::ApplyUpdate(payload)).await;
                }
            });
        }

        Ok(Self {
            doc,

            tag: args.origin,

            session: args.session,

            _subscription: subscription,

            _marker: PhantomData,
        })
    }
}

#[derive(Default, Deserialize, Debug, Serialize, Clone, PartialEq)]
struct RobotPosition {
    id: u8,
    x: f32,
    y: f32,
}

impl RobotPosition {
    #[allow(clippy::new_without_default)]
    fn new(id: u8, x: f32, y: f32) -> Self {
        RobotPosition { id, x, y }
    }

    fn add_x(&mut self, value: f32) {
        self.x += value;
    }

    fn add_y(&mut self, value: f32) {
        self.y += value;
    }
}

#[derive(Debug, Parser)]
struct Args {
    #[arg(short, long)]
    id: u8,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let session = zenoh::open(zenoh::Config::default()).await.unwrap();
    zenoh::init_log_from_env_or("info");

    let (owned_id, remote_id) = if args.id == 1 { (1, 2) } else { (2, 1) };

    let owned_ctx = DocumentContext::new(
        session.clone(),
        RobotId(owned_id as u8),
        DocumentTag::Owned(yrs::Origin::from(owned_id.to_string())),
    );
    let owned_actor = Document::<RobotPosition>::spawn(owned_ctx);

    let remote_ctx = DocumentContext::new(
        session.clone(),
        RobotId(remote_id as u8),
        DocumentTag::Remote(yrs::Origin::from(remote_id.to_string())),
    );
    let remote_actor = Document::<RobotPosition>::spawn(remote_ctx);

    // Give time for subscribers to be set up
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // Only initialize the LOCAL document - remote will be synced from the other instance
    owned_actor
        .tell(DocumentMessage::Set(RobotPosition::new(args.id, 0.0, 0.0)))
        .await?;

    const HELP_MESSAGE: &'static str = "Arrow up/down to update y position\n Arrow left/right to update x position\n p to print state\n q to quit\n";

    let print_help = || {
        crossterm::execute!(stdout(), crossterm::style::Print(HELP_MESSAGE))
            .expect("Error printing instructions.")
    };
    print_help();
    loop {
        if let Ok(crossterm::event::Event::Key(key)) = crossterm::event::read() {
            match key.code {
                KeyCode::Down | KeyCode::Up | KeyCode::Left | KeyCode::Right => {
                    let mut pos = owned_actor.ask(DocumentMessage::Get).await?.inner();
                    match key.code {
                        KeyCode::Down => pos.add_y(1.0),
                        KeyCode::Up => pos.add_y(-1.0),
                        KeyCode::Left => pos.add_x(-1.0),
                        KeyCode::Right => pos.add_x(1.0),
                        _ => unreachable!(),
                    };
                    owned_actor.tell(DocumentMessage::Set(pos)).await?;
                }
                KeyCode::Char('p') => {
                    let owned_pos = owned_actor.ask(DocumentMessage::Get).await?.inner();
                    let remote_pos = remote_actor.ask(DocumentMessage::Get).await?.inner();
                    println!("\nOwned: {:?}, Remote: {:?}", owned_pos, remote_pos);
                }
                KeyCode::Char('h') => print_help(),
                KeyCode::Char('q') => break,
                _ => {}
            }
        }
    }
    Ok(())
}
