# SuperNova

SuperNova is a terminal-based peer-to-peer (P2P) messenger written in Rust. The application enables secure, encrypted text communication directly between nodes without relying on centralized servers, databases, or third-party cloud infrastructure. The interface is built using a minimalist terminal user interface with a classic monochrome layout.

---

## Core Architecture & Mechanics

The system operates strictly on a client-to-client basis. The network lifecycle of a session proceeds as follows:

1. **Initialization:** Upon execution, the application binds to a local network port (default: `9099`) and initializes a `TcpListener` to await incoming connection streams.
2. **Cryptographic Handshake:** When a connection is initiated via the `/connect` command, the two nodes establish a raw TCP stream and execute an asymmetric key exchange using the **X25519 (Diffie-Hellman)** protocol.
3. **Secret Derivation:** Both nodes compute a shared session secret locally based on the exchanged public keys. The raw secret key is never transmitted across the network.
4. **Symmetric Encryption:** All subsequent message payloads are encrypted and decrypted using the **ChaCha20-Poly1305** authenticated encryption scheme. Each message packet incorporates a unique, automatically incremented cryptographic nonce to prevent replay attacks.
5. **Volatile Memory Management:** The application bypasses the local storage drive entirely. Message history and session keys reside strictly in volatile memory (RAM). Upon triggering the panic exit, the system invokes the `Zeroize` trait to overwrite the designated memory buffers with zeroes before terminating the process.

---

## Technical Stack

* **Language:** Rust
* **Asynchronous Runtime:** Tokio
* **Terminal UI Engine:** Ratatui & Crossterm
* **Cryptographic Primitives:** X25519, ChaCha20-Poly1305, Zeroize

---

## Key Advantages

* **Serverless Architecture:** Complete elimination of intermediary servers removes single points of failure and central data logging hazards.
* **End-to-End Encryption (E2EE):** Total mitigation of Man-in-the-Middle (MITM) attacks. Intercepted network packets render as non-deterministic cryptographic noise.
* **Memory-Level Security:** Ephemeral data architecture ensures that local forensic imaging cannot extract past conversations from the disk.
* **Low Resource Footprint:** Optimized binary size and minimal CPU/RAM overhead typical of native Rust terminal utilities.

---

## Installation & Compilation

Compiling SuperNova requires the standard Rust toolchain (`cargo` and `rustc`).

### General Build
[Downoload actual version from releases](https://github.com/Parnich/SuperNova/releases)

or

1. Clone the source repository:
```bash
git clone https://github.com/Parnich/supernova.git
cd supernova
```
2. Compile the production binary with full optimizations:
```bash
cargo build --release
```
The compiled binary will be located at `./target/release/supernova`

### Linux Deployment Guide
To build and execute SuperNova on Linux distributions (Ubuntu, Debian, Fedora, Arch Linux), ensure the core development utilities are installed.
1. Install Dependencies:
   - For Debian/Ubuntu:
     ```bash
      sudo apt update
      sudo apt install build-essential
      ```
   - For Fedora/RHEL:
     ```bash
     sudo dnf groupinstall "Development Tools"
     ```
   - For Arch Linux:
     ```bash
     sudo pacman -S base-devel
     ```
2. Execution:  
Launch the application directly from your terminal emulator:
```bash
./target/release/supernova
```
_Note: Ensure your system firewall (e.g., `ufw` or `firewalld`) allows traffic through your chosen port if you are testing inside a local area network_

## User Manual & Interface Controls
### Initialization
Run the application and input your desired cryptographic alias (nickname) at the initial prompt, then press Enter to open the communication interface.

## Terminal Commands & Hotkeys
All systemic operations are managed through input field commands or direct hardware key sequences.
| Command / Key | Function |
|---|---|
| /connect \<IP\>:\<PORT\> |	Establishes a direct TCP pipe to a remote node (e.g., /connect 192.168.1.50:9099). |
| /status |	Appends technical session diagnostic data directly to the chat log, including connection status and public key hex-fingerprints. |
| Up / Down	| Scrolls the main text buffer up or down by a single line. |
| PageUp / PageDown |	Scrolls the main text buffer up or down by 10 lines for rapid log parsing. |
| Esc |	Instantly overwrites cryptographic structures in memory with zeroes and terminates the process. |

## Global Connectivity & NAT Traversal
Because SuperNova relies on direct TCP sockets, connections attempted across different external networks will be blocked by standard residential router firewalls and Network Address Translation (NAT). Use one of the following protocols to route traffic globally.
### Method 1: Network Tunneling via ngrok
This method establishes a secure, public-facing TCP relay endpoint without altering router settings or requiring a static public IP address.
1. Install and authenticate the ngrok agent on the host system.
2. Initialize a TCP tunnel mapped to the default SuperNova listener port:
   ```bash
   ngrok tcp 9099
   ```
3. The ngrok dashboard will display an active forwarding address (e.g., `tcp://0.tcp.ngrok.io:12345`).
4. Copy the address string without the protocol prefix: `0.tcp.ngrok.io:12345`
5. The remote peer can now connect to your node by executing the following command in their terminal interface:
   ```bash
   /connect 0.tcp.ngrok.io:12345
   ```
_Security Note: The ngrok proxy layer only intercepts encrypted ChaCha20-Poly1305 payloads. Decryption keys are confined strictly to the endpoints._
### Method 2: Virtual Local Area Network via Hamachi
This method establishes a permanent software-defined Virtual LAN (VLAN) tunnel between two distinct nodes.
1. Install the LogMeIn Hamachi client on both deployment targets.
2. Node A creates a new virtual network mesh ID and configures an access password.
3. Node B joins the designated network mesh using those credentials.
4. Note the virtual IPv4 address assigned to your peer within the Hamachi client interface (e.g., `25.45.102.12`).
5. Run SuperNova on both machines. Target your peer using their virtual IP coordinates:
  ```bash
  /connect 25.45.102.12:9099
  ```
