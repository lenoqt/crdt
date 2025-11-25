mod actor;
mod registry;
mod types;

use clap::Parser;
use crossterm::event::{Event, KeyCode};
use futures_util::stream::StreamExt;

use crate::actor::DocumentMessage;
use crate::registry::DocumentRegistry;
use crate::types::{CrdtError, RobotId};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use crossterm::{cursor, execute, terminal::{Clear, ClearType}};
use std::io::{stdout, Write};
use tokio::time::{interval, Duration};

#[derive(Debug, Parser)]
struct Args {
    #[arg(short, long)]
    id: u8,
}

#[tokio::main]
async fn main() -> Result<(), CrdtError> {
    let args = Args::parse();
    let session = zenoh::open(zenoh::Config::default())
        .await
        .map_err(CrdtError::Zenoh)?;
    zenoh::init_log_from_env_or("info");

    let owned_id = RobotId(args.id);

    let registry = DocumentRegistry::builder()
        .session(session)
        .id(owned_id)
        .build()
        .await?;

    enable_raw_mode()?;
    let result = run_tui(registry).await;
    disable_raw_mode()?;
    
    result
}

async fn run_tui(registry: DocumentRegistry) -> Result<(), CrdtError> {
    let mut reader = crossterm::event::EventStream::new();
    let owned_actor = registry.get_owned().await;
    let mut refresh_interval = interval(Duration::from_millis(500));

    loop {
        tokio::select! {
            _ = refresh_interval.tick() => {
                let mut stdout = stdout();
                execute!(stdout, Clear(ClearType::All), cursor::MoveTo(0, 0))?;
                
                let owned_pos = owned_actor.ask(DocumentMessage::Get).await.map_err(|e| CrdtError::Actor(format!("{:?}", e)))?.inner();
                writeln!(stdout, "Owned: {}\r", owned_pos)?;
                for (id, actor) in registry.get_all_remotes().await {
                    let remote_pos = actor.ask(DocumentMessage::Get).await.map_err(|e| CrdtError::Actor(format!("{:?}", e)))?.inner();
                    writeln!(stdout, "Remote {}: {}\r", id, remote_pos)?;
                }

                let stats = registry.get_stats();
                let (sent, recv, msg_sent, msg_recv) = stats.get_stats();
                writeln!(stdout, "\n--- Network Stats ---\r")?;
                writeln!(stdout, "Sent: {} bytes ({} msgs)\r", sent, msg_sent)?;
                writeln!(stdout, "Recv: {} bytes ({} msgs)\r", recv, msg_recv)?;
                writeln!(stdout, "Total: {} bytes\r", sent + recv)?;

                writeln!(stdout, "\nArrows=move | q=quit")?;
                stdout.flush()?;
            }
            
            Some(Ok(event)) = reader.next() => {
                if let Event::Key(key) = event {
                    match key.code {
                        KeyCode::Down | KeyCode::Up | KeyCode::Left | KeyCode::Right => {
                            let mut pos = owned_actor.ask(DocumentMessage::Get).await.map_err(|e| CrdtError::Actor(format!("{:?}", e)))?.inner();
                            match key.code {
                                KeyCode::Down => pos.add_y(1.0),
                                KeyCode::Up => pos.add_y(-1.0),
                                KeyCode::Left => pos.add_x(-1.0),
                                KeyCode::Right => pos.add_x(1.0),
                                _ => unreachable!(),
                            };
                            owned_actor.tell(DocumentMessage::Set(pos)).await.map_err(|e| CrdtError::Actor(format!("{:?}", e)))?;
                        }
                        KeyCode::Char('p') => {
                            let owned_pos = owned_actor.ask(DocumentMessage::Get).await.map_err(|e| CrdtError::Actor(format!("{:?}", e)))?.inner();
                            println!("\nOwned: {}", owned_pos);
                            for (id, actor) in registry.get_all_remotes().await {
                                let remote_pos = actor.ask(DocumentMessage::Get).await.map_err(|e| CrdtError::Actor(format!("{:?}", e)))?.inner();
                                println!("Remote {}: {}", id, remote_pos);
                            }
                        }
                        KeyCode::Char('h') => {
                             const HELP_MESSAGE: &str = "Arrow up/down to update y position\n Arrow left/right to update x position\n p to print state\n q to quit\n";
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
