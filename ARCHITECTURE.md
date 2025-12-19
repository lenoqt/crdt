# Architecture Overview

This document provides an overview of the architecture of the CRDT (Conflict-free Replicated Data Type) collaborative demo application.

## 1. Introduction

The project is a peer-to-peer, real-time collaborative application built in Rust. It demonstrates how multiple instances of the application can synchronize the state of two "robot" entities without a central server. Each user runs an instance of the application and can control one robot, while seeing the other robot's movements in real-time as they are updated by another user.

## 2. Core Components

The application is built around a few key libraries that handle concurrency, data synchronization, and networking.

### `kameo` (Actor Framework)

The application uses the `kameo` actor framework to manage concurrency and state. The core of the application is the `Document` actor, which encapsulates the state of a single robot.

*   **`Document` Actor:** Each robot's state is managed by a dedicated `Document` actor. This actor is responsible for holding the CRDT document for the robot's position, applying local and remote changes, and interacting with the networking layer.

### `yrs` (CRDTs)

Conflict-free Replicated Data Types (CRDTs) are the foundation of the synchronization logic. The `yrs` library (a Rust port of the popular Y.js library) provides the CRDT implementation.

*   **`RobotPosition` struct:** The state of each robot (its ID and x/y coordinates) is stored in a `RobotPosition` struct. This struct is serialized to JSON and stored in a `yrs::Array` within a `yrs::Doc`.
*   **State Synchronization:** `yrs` handles the merging of updates from different peers, ensuring that the state of the document converges to the same value across all instances, even when changes are made concurrently.

### `Kameo Remote` (Networking)

The application uses Kameo's built-in remote actor capabilities for networking and peer discovery. This provides a decentralized way for actors to communicate across different nodes.

*   **Discovery:** The `DocumentRegistry` uses Kameo's DHT-based discovery mechanism (`RemoteActorRef::lookup_all`) to find other `Document` actors on the network registered with the name "crdt_document".
*   **Messaging:** Updates are broadcasted by sending `DocumentMessage::ApplyUpdate` messages directly to known remote actor references.
*   **Remote Messages:** The `DocumentMessage` enum is annotated with `#[remote_message]` to ensure proper serialization and routing between nodes.

### `crossterm` and `clap` (CLI)

These libraries are used to create the command-line user interface.

*   **`clap`:** Parses the `--id` command-line argument, which determines which robot the user controls.
*   **`crossterm`:** Captures keyboard events (arrow keys for movement) and controls the terminal output.

## 3. Application Flow

1.  **Startup:** The application is started with an ID (1 or 2), e.g., `crdt --id 1`.
2.  **Bootstrap:** Kameo's remote system is bootstrapped to join the distributed network.
3.  **Actor Creation:** The `DocumentRegistry` creates an **"owned" actor** for the local robot and registers it with the name "crdt_document".
4.  **Discovery:** A background task in `DocumentRegistry` continuously looks up other actors named "crdt_document". When a new peer is found, it is added to the registry.
5.  **User Input:** The application listens for keyboard input. When an arrow key is pressed, it sends a message to the `owned_actor` to update its position.
6.  **Update Propagation:**
    *   The `owned_actor` updates its local CRDT document.
    *   An observer triggers and encodes the update.
    *   The `DocumentRegistry` broadcasts this update to all known remote peers by sending `DocumentMessage::ApplyUpdate`.
7.  **Update Reception:** Remote actors receive the `ApplyUpdate` message.
8.  **State Convergence:** The remote actor applies the update to its local CRDT document, ensuring all peers see the same state.

## 4. Data Flow Diagram

Here is a simplified data flow diagram for a scenario where User 1 (with `--id 1`) moves their robot:

```
+----------------+      +----------------+
| User 1         |      | User 2         |
| (Instance --id 1)|      | (Instance --id 2)|
+----------------+      +----------------+
       |                        |
       | 1. Moves Robot 1       |
       v                        |
+----------------+              |
| Owned Actor 1  |              |
| (Instance 1)   |              |
+----------------+              |
       |                        |
       | 2. Update Observer     |
       v                        |
+----------------+              |
| DocumentRegistry|             |
| (Instance 1)   |              |
+----------------+              |
       |                        |
       | 3. Broadcasts Update   |
       |    via Kameo Remote    |
       v                        v
+-------------------------------------------------------------+
| Kameo Distributed Network (DHT / P2P)                       |
+-------------------------------------------------------------+
       |                        |
       |                        | 4. Receives ApplyUpdate msg
       |                        v
       |                +----------------+
       |                | Remote Actor 1 |
       |                | (Instance 2)   |
       |                +----------------+
       |                        |
       |                        | 5. Updates local state
       |                        v
       |                (Screen updates)
```

This architecture creates a robust and decentralized system for real-time collaboration, where each peer is responsible for its own state and communicates directly with others.
