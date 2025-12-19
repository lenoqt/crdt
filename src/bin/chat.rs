use clap::Parser;
use crdt::actor::{Document, DocumentContext, GetAllData, SetData};
use crdt::types::{ChatMessage, RobotId};
use kameo::actor::Spawn;
use kameo::prelude::*;
use std::io::{self, Write};
use tokio::io::{AsyncBufReadExt, BufReader};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// ID of the user (u8)
    #[arg(short, long)]
    id: u8,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let my_id = RobotId(args.id);

    println!("Starting Chat App for User {}", my_id);

    // Bootstrap Kameo
    kameo::remote::bootstrap()?;

    let initial_state = ChatMessage {
        sender_id: args.id,
        content: "Joined the chat".to_string(),
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs(),
    };

    let context = DocumentContext {
        id: args.id.to_string(),
    };
    let chat_actor = Document::<ChatMessage>::spawn(context);

    // Initial message
    let key = format!(
        "{}-{}-{}",
        initial_state.timestamp,
        initial_state.sender_id,
        initial_state.content.len()
    );
    chat_actor
        .tell(SetData(key.clone(), initial_state.clone()))
        .send()
        .await?;

    // Input loop
    let mut stdin = BufReader::new(tokio::io::stdin());
    let mut line = String::new();

    println!("Type a message and press Enter:");

    loop {
        print!("> ");
        io::stdout().flush()?;

        line.clear();
        let bytes_read = stdin.read_line(&mut line).await?;
        if bytes_read == 0 {
            break; // EOF
        }

        let content = line.trim().to_string();
        if content.is_empty() {
            continue;
        }

        if content == "/quit" {
            break;
        }

        if content == "/list" {
            let response = chat_actor.ask(GetAllData).await?;
            let messages = response.data;
            println!("--- Chat History ---");
            // Sort by key (timestamp)
            let mut sorted_msgs: Vec<_> = messages.into_iter().collect();
            sorted_msgs.sort_by_key(|(k, _)| k.clone());
            for (_id, msg) in sorted_msgs {
                println!("{}", msg);
            }
            println!("--------------------");
            continue;
        }

        let msg = ChatMessage {
            sender_id: args.id,
            content: content.clone(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs(),
        };

        let key = format!("{}-{}-{}", msg.timestamp, msg.sender_id, msg.content.len());

        chat_actor.tell(SetData(key, msg)).send().await?;
    }

    Ok(())
}
