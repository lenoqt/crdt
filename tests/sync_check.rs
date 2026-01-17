use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;

fn start_chat(id: u8) -> Child {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_chat2"));
    cmd.arg("--id")
        .arg(id.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());

    cmd.spawn().expect("Failed to start chat process")
}

#[test]
fn test_chat_sync() {
    // Compile the binary first if running via cargo test,
    // but typically cargo test builds binaries if they are needed purely for integration tests?
    // Actually, usually we need to ensure `cargo build --bin chat` is run or rely on `env!("CARGO_BIN_EXE_chat")` which cargo test sets up for us if we have [dev-dependencies].
    // Note: env!("CARGO_BIN_EXE_<name>") is available in integration tests.

    let mut peer1 = start_chat(1);
    let mut peer2 = start_chat(2);

    // Give them time to discover each other (simulated)
    thread::sleep(Duration::from_secs(5));

    // Peer 1 sends a message
    if let Some(stdin) = peer1.stdin.as_mut() {
        writeln!(stdin, "Hello from Peer 1").expect("Failed to write to peer1");
    }

    // Give time for propagation
    thread::sleep(Duration::from_secs(5));

    // Check Peer 2's list
    if let Some(stdin) = peer2.stdin.as_mut() {
        writeln!(stdin, "/list").expect("Failed to write to peer2");
    }

    // Read Peer 2's output
    let stdout = peer2.stdout.as_mut().expect("Failed to open stdout");
    let reader = BufReader::new(stdout);

    let mut found = false;
    // We strictly just want to see if the message appears in the output
    // Read lines with a timeout or just read a fixed amount
    // Since we can't easily do non-blocking read with std::io, we'll loop a bit?
    // Or just look at the first few lines of output after /list.

    // Note: The process will output the list immediately.
    for line in reader.lines() {
        let l = line.expect("Failed to read line");
        println!("Peer2 output: {}", l);
        if l.contains("Hello from Peer 1") {
            found = true;
            break;
        }
        if l.contains("--------------------") {
            // End of list
            break;
        }
    }

    // Cleanup
    let _ = peer1.kill();
    let _ = peer2.kill();

    assert!(found, "Peer 2 did not receive the message from Peer 1");
}
