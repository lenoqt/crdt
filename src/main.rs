use clap::Parser;
use crossterm::event::{Event, KeyCode};
use futures_util::stream::StreamExt;
use kameo::actor::Spawn;

use crdt::actor::{Document, DocumentContext, GetAllData, SetData};
use crdt::types::{CrdtError, RobotId, RobotPosition};

#[derive(Debug, Parser)]
struct Args {
    #[arg(short, long)]
    id: u8,
}

#[tokio::main]
async fn main() -> Result<(), CrdtError> {
    let args = Args::parse();

    // Bootstrap Kameo distributed actors
    kameo::remote::bootstrap().map_err(|e| CrdtError::Other(format!("Bootstrap failed: {}", e)))?;

    let owned_id = RobotId(args.id);
    let initial_pos = RobotPosition::new(args.id, 0.0, 0.0);

    // Use string ID for DocumentContext
    let context = DocumentContext {
        id: args.id.to_string(),
    };
    let actor_ref = Document::<RobotPosition>::spawn(context);

    // Initial set
    actor_ref
        .tell(SetData(owned_id.to_string(), initial_pos))
        .send()
        .await
        .map_err(|e| CrdtError::ActorSend(e.to_string()))?;

    const HELP_MESSAGE: &str = "Arrow up/down to update y position\n Arrow left/right to update x position\n p to print state\n d to discover peers (auto)\n q to quit\n";
    println!("{}", HELP_MESSAGE);

    run_tui(actor_ref, owned_id).await?;

    Ok(())
}

async fn run_tui(
    actor: kameo::actor::ActorRef<Document<RobotPosition>>,
    owned_id: RobotId,
) -> Result<(), CrdtError> {
    let mut reader = crossterm::event::EventStream::new();

    loop {
        tokio::select! {
            Some(Ok(event)) = reader.next() => {
                if let Event::Key(key) = event {
                    match key.code {
                        KeyCode::Down | KeyCode::Up | KeyCode::Left | KeyCode::Right |
                        KeyCode::Char('i') | KeyCode::Char('j') | KeyCode::Char('k') | KeyCode::Char('l') => {
                            let response = actor.ask(GetAllData).await.map_err(|e| CrdtError::ActorSend(e.to_string()))?;
                            let all_pos = response.data;
                            if let Some(mut pos) = all_pos.get(&owned_id.to_string()).cloned() {
                                match key.code {
                                    KeyCode::Down | KeyCode::Char('k') => pos.add_y(1.0),
                                    KeyCode::Up | KeyCode::Char('i') => pos.add_y(-1.0),
                                    KeyCode::Left | KeyCode::Char('j') => pos.add_x(-1.0),
                                    KeyCode::Right | KeyCode::Char('l') => pos.add_x(1.0),
                                    _ => unreachable!(),
                                };
                                actor.tell(SetData(owned_id.to_string(), pos)).send().await.map_err(|e| CrdtError::ActorSend(e.to_string()))?;
                            }
                        }
                        KeyCode::Char('p') => {
                            let response = actor.ask(GetAllData).await.map_err(|e| CrdtError::ActorSend(e.to_string()))?;
                            let all_pos = response.data;
                            println!("\n--- Robot Positions ---");
                            for (id_str, pos) in all_pos {
                                if id_str == owned_id.to_string() {
                                    println!("Owned: {}", pos);
                                } else {
                                    println!("Remote {}: {}", id_str, pos);
                                }
                            }
                            println!("-----------------------");
                        }
                        KeyCode::Char('d') => {
                            // Discovery is automatic now
                            println!("Discovery is automatic.");
                        }
                        KeyCode::Char('h') => {
                             const HELP_MESSAGE: &str = "Arrow up/down to update y position\n Arrow left/right to update x position\n p to print state\n d to discover peers (auto)\n q to quit\n";
                             println!("{}", HELP_MESSAGE);
                        }
                        KeyCode::Char('q') => break,
                        _ => {}
                    }
                }
            }
        }
    }
    Ok(())
}
