use crossterm::{
    event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures::StreamExt;
use rand::RngCore;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    widgets::{Paragraph, Wrap},
    Terminal,
};
use std::{io, net::SocketAddr};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use zeroize::{Zeroize, ZeroizeOnDrop};

#[derive(Zeroize, ZeroizeOnDrop)]
struct SecureBuffer {
    data: Vec<u8>,
}

enum AppState {
    Listening,
    Connected {
        peer_addr: SocketAddr,
        peer_name: String,
        tx: tokio::sync::mpsc::Sender<Vec<u8>>,
    },
}

enum AppEvent {
    Terminal(KeyEvent),
    NetworkNewConnection(tokio::net::TcpStream, SocketAddr),
    NetworkMessage(String),
    NetworkDisconnected,
    Error(String),
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app_state = AppState::Listening;
    let mut input_text = String::new();
    let mut chat_history: Vec<String> = vec![];

    let listener = TcpListener::bind("0.0.0.0:9099").await?;

    let mut reader = EventStream::new();
    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<AppEvent>(100);

    let net_tx = event_tx.clone();
    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    let _ = net_tx.send(AppEvent::NetworkNewConnection(stream, addr)).await;
                }
                Err(e) => {
                    let _ = net_tx.send(AppEvent::Error(e.to_string())).await;
                }
            }
        }
    });

    let term_tx = event_tx.clone();
    tokio::spawn(async move {
        loop {
            if let Some(Ok(Event::Key(key))) = reader.next().await {
                let _ = term_tx.send(AppEvent::Terminal(key)).await;
            }
        }
    });

    loop {
        terminal.draw(|f| {
            let size = f.size();

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),
                    Constraint::Length(1),
                    Constraint::Min(1),
                    Constraint::Length(1),
                ])
                .split(size);

            let remaining_width = size.width as usize;
            let mut header_string = "── SuperNova ──".to_string();
            if header_string.len() < remaining_width {
                let dashes = "─".repeat(remaining_width - header_string.len());
                header_string.push_str(&dashes);
            }

            let header = Paragraph::new(header_string)
                .style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD));
            f.render_widget(header, chunks[0]);

            let status_string = match &app_state {
                AppState::Listening => "Listening for incoming connections on port 9099...".to_string(),
                AppState::Connected { peer_name, peer_addr, .. } => format!("Connected with @{} ({})", peer_name, peer_addr),
            };
            let status = Paragraph::new(status_string)
                .style(Style::default().fg(Color::Green));
            f.render_widget(status, chunks[1]);

            let history_content = chat_history.join("\n");
            let chat_box = Paragraph::new(history_content)
                .style(Style::default().fg(Color::Green))
                .wrap(Wrap { trim: true });
            f.render_widget(chat_box, chunks[2]);

            let input_display = format!("> {}", input_text);
            let input_box = Paragraph::new(input_display)
                .style(Style::default().fg(Color::Green));
            f.render_widget(input_box, chunks[3]);
        })?;

        if let Some(event) = event_rx.recv().await {
            match event {
                AppEvent::Terminal(key) => {
                    if key.kind == KeyEventKind::Press {
                        if key.code == KeyCode::Esc {
                            input_text.zeroize();
                            chat_history.iter_mut().for_each(|s| s.zeroize());
                            break;
                        }

                        match key.code {
                            KeyCode::Char(c) => {
                                input_text.push(c);
                            }
                            KeyCode::Backspace => {
                                input_text.pop();
                            }
                            KeyCode::Enter => {
                                if !input_text.is_empty() {
                                    if input_text.starts_with("/connect ") {
                                        let addr_str = input_text.trim_start_matches("/connect ").to_string();
                                        chat_history.push(format!("[System]: Connecting to {}...", addr_str));
                                        input_text.clear();
                                        let connect_tx = event_tx.clone();
                                        tokio::spawn(async move {
                                            if let Ok(addr) = addr_str.parse::<SocketAddr>() {
                                                match tokio::net::TcpStream::connect(addr).await {
                                                    Ok(stream) => {
                                                        let _ = connect_tx.send(AppEvent::NetworkNewConnection(stream, addr)).await;
                                                    }
                                                    Err(e) => {
                                                        let _ = connect_tx.send(AppEvent::Error(e.to_string())).await;
                                                    }
                                                }
                                            } else {
                                                let _ = connect_tx.send(AppEvent::Error("Invalid address format".to_string())).await;
                                            }
                                        });
                                    } else {
                                        match &app_state {
                                            AppState::Connected { tx, .. } => {
                                                let _ = tx.send(input_text.as_bytes().to_vec()).await;
                                                chat_history.push(format!("[You]: {}", input_text));
                                                input_text.clear();
                                            }
                                            AppState::Listening => {
                                                chat_history.push("[System]: Not connected. Use /connect <ip>:<port>".to_string());
                                                input_text.clear();
                                            }
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                AppEvent::NetworkNewConnection(stream, addr) => {
                    match app_state {
                        AppState::Listening => {
                            let (net_send_tx, mut net_send_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(100);
                            let main_tx = event_tx.clone();

                            app_state = AppState::Connected {
                                peer_addr: addr,
                                peer_name: "Peer".to_string(),
                                tx: net_send_tx,
                            };
                            chat_history.push(format!("[System]: Successfully connected with [{}].", addr));

                            let (mut reader, mut writer) = stream.into_split();
                            tokio::spawn(async move {
                                let mut read_buf = [0u8; 512];
                                loop {
                                    tokio::select! {
                                        res = reader.read_exact(&mut read_buf) => {
                                            match res {
                                                Ok(_) => {
                                                    let packet_type = read_buf[0];
                                                    if packet_type == 1 {
                                                        let len = u16::from_be_bytes([read_buf[1], read_buf[2]]) as usize;
                                                        if len <= 509 {
                                                            if let Ok(msg) = String::from_utf8(read_buf[3..3+len].to_vec()) {
                                                                let _ = main_tx.send(AppEvent::NetworkMessage(msg)).await;
                                                            }
                                                        }
                                                    }
                                                }
                                                Err(_) => {
                                                    let _ = main_tx.send(AppEvent::NetworkDisconnected).await;
                                                    break;
                                                }
                                            }
                                        }
                                        Some(msg_bytes) = net_send_rx.recv() => {
                                            let mut write_buf = [0u8; 512];
                                            write_buf[0] = 1;
                                            let len = msg_bytes.len().min(509);
                                            let len_bytes = (len as u16).to_be_bytes();
                                            write_buf[1] = len_bytes[0];
                                            write_buf[2] = len_bytes[1];
                                            write_buf[3..3+len].copy_from_slice(&msg_bytes[..len]);
                                            rand::thread_rng().fill_bytes(&mut write_buf[3+len..]);
                                            if writer.write_all(&write_buf).await.is_err() {
                                                let _ = main_tx.send(AppEvent::NetworkDisconnected).await;
                                                break;
                                            }
                                        }
                                    }
                                }
                            });
                        }
                        AppState::Connected { .. } => {
                            drop(stream);
                        }
                    }
                }
                AppEvent::NetworkMessage(msg) => {
                    let name = match &app_state {
                        AppState::Connected { peer_name, .. } => peer_name.clone(),
                        _ => "Peer".to_string(),
                    };
                    chat_history.push(format!("[{}]: {}", name, msg));
                }
                AppEvent::NetworkDisconnected => {
                    chat_history.push("[System]: Connection closed by peer.".to_string());
                    app_state = AppState::Listening;
                }
                AppEvent::Error(err) => {
                    chat_history.push(format!("[Error]: {}", err));
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}