mod actor;
mod registry;
mod types;

use clap::Parser;
use crossterm::event::{Event, KeyCode};
use futures_util::stream::StreamExt;
use kameo::actor::Spawn;

use std::time::Duration;

use crate::actor::{Document, DocumentContext, DocumentMessage, DocumentTag};
use crate::registry::DocumentRegistry;
use crate::types::{PresenceMessage, RobotId, RobotPosition};

const DISCOVERY_TOPIC: &str = "crdt/discovery";

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

    let owned_id = RobotId(args.id);
    let owned_ctx = DocumentContext {
        session: session.clone(),
        robot_id: owned_id,
        origin: DocumentTag::Owned(yrs::Origin::from(owned_id.to_string())),
        initial_state: Some(RobotPosition::new(args.id, 0.0, 0.0)),
    };
    let owned_actor = Document::<RobotPosition>::spawn(owned_ctx);

    let registry = DocumentRegistry::new();

    // Discovery
    let discovery_sub = session.declare_subscriber(DISCOVERY_TOPIC).await.unwrap();
    let session_clone = session.clone();
    let args_id = args.id;
    tokio::spawn(async move {
        loop {
            let msg = PresenceMessage::Presence(args_id);
            let payload = serde_json::to_string(&msg).unwrap();
            session_clone.put(DISCOVERY_TOPIC, payload).await.unwrap();
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });

    const HELP_MESSAGE: &str = "Arrow up/down to update y position\n Arrow left/right to update x position\n p to print state\n q to quit\n";
    println!("{}", HELP_MESSAGE);

    let mut reader = crossterm::event::EventStream::new();

    loop {
        tokio::select! {
            Some(Ok(event)) = reader.next() => {
                if let Event::Key(key) = event {
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
                            println!("\nOwned: {:?}", owned_pos);
                            for (id, actor) in registry.get_all_remotes().await {
                                let remote_pos = actor.ask(DocumentMessage::Get).await?.inner();
                                println!("Remote {}: {:?}", id, remote_pos);
                            }
                        }
                        KeyCode::Char('h') => println!("{}", HELP_MESSAGE),
                        KeyCode::Char('q') => break,
                        _ => {}
                    }
                }
            },
            Ok(sample) = discovery_sub.recv_async() => {
                if let Ok(msg_str) = std::str::from_utf8(&sample.payload().to_bytes().to_vec()) {
                    if let Ok(PresenceMessage::Presence(id)) = serde_json::from_str(msg_str) {
                        let remote_id = RobotId(id);
                        if remote_id != owned_id {
                            registry.add_remote(remote_id, session.clone()).await;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}
