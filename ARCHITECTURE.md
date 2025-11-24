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

### `zenoh` (Networking)

The `zenoh` framework is used as a decentralized, peer-to-peer pub/sub communication layer. It allows the different instances of the application to broadcast and receive updates without a central message broker.

*   **Topic Structure:** The application uses a structured topic semantic for communication: `robot_id/position/sync`. For example, updates for robot 1 are published to the topic `1/position/sync`.
*   **Publish-Subscribe Model:**
    *   **Publishing:** An actor representing an "owned" robot (one controlled by the local user) publishes updates to its corresponding topic whenever its state changes.
    *   **Subscribing:** An actor representing a "remote" robot subscribes to the corresponding topic to receive updates from the peer controlling that robot.

### `crossterm` and `clap` (CLI)

These libraries are used to create the command-line user interface.

*   **`clap`:** Parses the `--id` command-line argument, which determines which robot the user controls.
*   **`crossterm`:** Captures keyboard events (arrow keys for movement) and controls the terminal output.

## 3. Application Flow

1.  **Startup:** The application is started with an ID (1 or 2), e.g., `crdt --id 1`.
2.  **Actor Creation:** The `main` function creates two `Document` actors:
    *   An **"owned" actor** for the robot corresponding to the provided ID. This actor is configured to publish updates.
    *   A **"remote" actor** for the other robot. This actor is configured to subscribe to updates.
3.  **User Input:** The application listens for keyboard input. When an arrow key is pressed, it sends a message to the `owned_actor` to update its position.
4.  **Update Publication:** The `owned_actor` updates its local CRDT document. This triggers an observer that encodes the changes into an update payload and publishes it to the corresponding `zenoh` topic (e.g., `1/position/sync`).
5.  **Update Reception:** On the other peer's machine, the `remote_actor` (which is subscribed to that topic) receives the update payload.
6.  **State Convergence:** The `remote_actor` applies the update to its local CRDT document, causing the position of the remote robot to be updated on the screen.

## 4. Data Flow Diagram

Here is a simplified data flow diagram for a scenario where User 1 (with `--id 1`) moves their robot:

```
+----------------+      +----------------+      +----------------+
| User 1         |      | User 2         |      | User 3         |
| (Instance --id 1)|      | (Instance --id 2)|      | (Instance --id 1)|
+----------------+      +----------------+      +----------------+
       |                        |                        |
       | 1. Moves Robot 1       |                        |
       v                        |                        |
+----------------+              |                        |
| Owned Actor 1  |              |                        |
| (Instance 1)   |              |                        |
+----------------+              |                        |
       |                        |                        |
       | 2. Publishes to topic  |                        |
       |    '1/position/sync'   |                        |
       v                        v                        v
+-------------------------------------------------------------+
| zenoh network                                               |
+-------------------------------------------------------------+
       |                        |                        |
       |                        | 3. Receives update     | 3. Receives update
       |                        v                        v
       |                +----------------+      +----------------+
       |                | Remote Actor 1 |      | Remote Actor 1 |
       |                | (Instance 2)   |      | (Instance 3)   |
       |                +----------------+      +----------------+
       |                        |                        |
       |                        | 4. Updates local state | 4. Updates local state
       |                        v                        v
       |                (Screen updates)          (Screen updates)
       |
       | (No subscription for owned document, no local update from network)
       |
```

This architecture creates a robust and decentralized system for real-time collaboration, where each peer is responsible for its own state and communicates directly with others.
