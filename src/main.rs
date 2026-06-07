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
    InputName,
    Listening {
        my_name: String,
    },
    IncomingRequest {
        my_name: String,
        peer_addr: SocketAddr,
        peer_name: String,
        stream: tokio::net::TcpStream,
    },
    Connected {
        my_name: String,
        peer_addr: SocketAddr,
        peer_name: String,
        tx: tokio::sync::mpsc::Sender<Vec<u8>>,
    },
}

enum AppEvent {
    Terminal(KeyEvent),
    NetworkIncomingAuth(tokio::net::TcpStream, SocketAddr, String),
    HandshakeSuccess(tokio::net::TcpStream, SocketAddr, String),
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

    let mut app_state = AppState::InputName;
    let mut input_text = String::new();
    let mut chat_history: Vec<String> = vec![];

    let listener = TcpListener::bind("0.0.0.0:9099").await?;

    let mut reader = EventStream::new();
    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<AppEvent>(100);

    let net_tx = event_tx.clone();
    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((mut stream, addr)) => {
                    let incoming_tx = net_tx.clone();
                    tokio::spawn(async move {
                        let mut req_buf = [0u8; 512];
                        if stream.read_exact(&mut req_buf).await.is_ok() {
                            if req_buf[0] == 2 {
                                let len = u16::from_be_bytes([req_buf[1], req_buf[2]]) as usize;
                                if len <= 509 {
                                    if let Ok(name) = String::from_utf8(req_buf[3..3+len].to_vec()) {
                                        let _ = incoming_tx.send(AppEvent::NetworkIncomingAuth(stream, addr, name)).await;
                                        return;
                                    }
                                }
                            }
                        }
                        drop(stream);
                    });
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

            let mut header_string = "── SuperNova ──".to_string();
            if header_string.len() < size.width as usize {
                let dashes = "─".repeat(size.width as usize - header_string.len());
                header_string.push_str(&dashes);
            }

            let header = Paragraph::new(header_string)
                .style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD));
            f.render_widget(header, chunks[0]);

            let status_string = match &app_state {
                AppState::InputName => "Set your identity before entering the grid...".to_string(),
                AppState::Listening { my_name } => format!("Listening on port 9099 as @{}", my_name),
                AppState::IncomingRequest { peer_name, peer_addr, .. } => format!("Authorization request from @{} ({})", peer_name, peer_addr),
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

            let input_prefix = match &app_state {
                AppState::InputName => "Enter Nickname: ",
                AppState::IncomingRequest { .. } => "Accept connection? (yes/no): ",
                _ => "> ",
            };
            let input_display = format!("{}{}", input_prefix, input_text);
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
                                    let current_state = std::mem::replace(&mut app_state, AppState::InputName);
                                    match current_state {
                                        AppState::InputName => {
                                            let name = input_text.trim().to_string();
                                            chat_history.push(format!("[System]: Identity secured as @{}.", name));
                                            app_state = AppState::Listening { my_name: name };
                                            input_text.clear();
                                        }
                                        AppState::Listening { my_name } => {
                                            if input_text.starts_with("/connect ") {
                                                let addr_str = input_text.trim_start_matches("/connect ").to_string();
                                                chat_history.push(format!("[System]: Sending transmission request to {}...", addr_str));
                                                input_text.clear();
                                                let connect_tx = event_tx.clone();
                                                let my_name_clone = my_name.clone();
                                                tokio::spawn(async move {
                                                    if let Ok(addr) = addr_str.parse::<SocketAddr>() {
                                                        match tokio::net::TcpStream::connect(addr).await {
                                                            Ok(mut stream) => {
                                                                let mut req_buf = [0u8; 512];
                                                                req_buf[0] = 2;
                                                                let name_bytes = my_name_clone.as_bytes();
                                                                let len = name_bytes.len().min(509);
                                                                let len_bytes = (len as u16).to_be_bytes();
                                                                req_buf[1] = len_bytes[0];
                                                                req_buf[2] = len_bytes[1];
                                                                req_buf[3..3+len].copy_from_slice(&name_bytes[..len]);
                                                                rand::thread_rng().fill_bytes(&mut req_buf[3+len..]);

                                                                if stream.write_all(&req_buf).await.is_ok() {
                                                                    let mut resp_buf = [0u8; 512];
                                                                    if stream.read_exact(&mut resp_buf).await.is_ok() {
                                                                        if resp_buf[0] == 3 && resp_buf[3] == 1 {
                                                                            let p_len = u16::from_be_bytes([resp_buf[1], resp_buf[2]]) as usize;
                                                                            if p_len > 1 && p_len <= 509 {
                                                                                if let Ok(p_name) = String::from_utf8(resp_buf[4..3+p_len].to_vec()) {
                                                                                    let _ = connect_tx.send(AppEvent::HandshakeSuccess(stream, addr, p_name)).await;
                                                                                    return;
                                                                                }
                                                                            }
                                                                        }
                                                                    }
                                                                }
                                                                let _ = connect_tx.send(AppEvent::Error("Connection rejected or handshake failed".to_string())).await;
                                                            }
                                                            Err(e) => {
                                                                let _ = connect_tx.send(AppEvent::Error(e.to_string())).await;
                                                            }
                                                        }
                                                    } else {
                                                        let _ = connect_tx.send(AppEvent::Error("Invalid address format".to_string())).await;
                                                    }
                                                });
                                                app_state = AppState::Listening { my_name };
                                            } else {
                                                chat_history.push("[System]: Use /connect <ip>:<port> to link with a peer.".to_string());
                                                input_text.clear();
                                                app_state = AppState::Listening { my_name };
                                            }
                                        }
                                        AppState::IncomingRequest { my_name, peer_addr, peer_name, mut stream } => {
                                            let answer = input_text.trim().to_lowercase();
                                            input_text.clear();
                                            if answer == "yes" {
                                                let mut resp_buf = [0u8; 512];
                                                resp_buf[0] = 3;
                                                let name_bytes = my_name.as_bytes();
                                                let len = (name_bytes.len() + 1).min(509);
                                                let len_bytes = (len as u16).to_be_bytes();
                                                resp_buf[1] = len_bytes[0];
                                                resp_buf[2] = len_bytes[1];
                                                resp_buf[3] = 1;
                                                resp_buf[4..4+name_bytes.len()].copy_from_slice(name_bytes);
                                                rand::thread_rng().fill_bytes(&mut resp_buf[4+name_bytes.len()..]);

                                                if stream.write_all(&resp_buf).await.is_ok() {
                                                    let (net_send_tx, mut net_send_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(100);
                                                    let main_tx = event_tx.clone();
                                                    let p_name = peer_name.clone();
                                                    let p_addr = peer_addr;

                                                    app_state = AppState::Connected {
                                                        my_name: my_name.clone(),
                                                        peer_addr: p_addr,
                                                        peer_name: p_name,
                                                        tx: net_send_tx,
                                                    };
                                                    chat_history.push("[System]: Secure channel established.".to_string());

                                                    let (mut reader, mut writer) = stream.into_split();
                                                    tokio::spawn(async move {
                                                        let mut read_buf = [0u8; 512];
                                                        loop {
                                                            tokio::select! {
                                                                res = reader.read_exact(&mut read_buf) => {
                                                                    match res {
                                                                        Ok(_) => {
                                                                            if read_buf[0] == 1 {
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
                                                } else {
                                                    chat_history.push("[System]: Failed to send confirmation.".to_string());
                                                    app_state = AppState::Listening { my_name };
                                                }
                                            } else {
                                                let mut resp_buf = [0u8; 512];
                                                resp_buf[0] = 3;
                                                resp_buf[3] = 0;
                                                let mut s = stream;
                                                tokio::spawn(async move {
                                                    let _ = s.write_all(&resp_buf).await;
                                                });
                                                chat_history.push("[System]: Connection declined.".to_string());
                                                app_state = AppState::Listening { my_name };
                                            }
                                        }
                                        AppState::Connected { my_name, peer_addr, peer_name, tx } => {
                                            let _ = tx.send(input_text.as_bytes().to_vec()).await;
                                            chat_history.push(format!("[You]: {}", input_text));
                                            input_text.clear();
                                            app_state = AppState::Connected { my_name, peer_addr, peer_name, tx };
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                AppEvent::NetworkIncomingAuth(stream, addr, peer_name) => {
                    let current_state = std::mem::replace(&mut app_state, AppState::InputName);
                    match current_state {
                        AppState::Listening { my_name } => {
                            chat_history.push(format!("[System]: @{} wants to link up. Respond with 'yes' or 'no'.", peer_name));
                            app_state = AppState::IncomingRequest {
                                my_name,
                                peer_addr: addr,
                                peer_name,
                                stream,
                            };
                        }
                        other => {
                            let mut resp_buf = [0u8; 512];
                            resp_buf[0] = 3;
                            resp_buf[3] = 0;
                            let mut s = stream;
                            tokio::spawn(async move {
                                let _ = s.write_all(&resp_buf).await;
                            });
                            app_state = other;
                        }
                    }
                }
                AppEvent::HandshakeSuccess(stream, addr, peer_name) => {
                    let current_state = std::mem::replace(&mut app_state, AppState::InputName);
                    if let AppState::Listening { my_name } = current_state {
                        let (net_send_tx, mut net_send_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(100);
                        let main_tx = event_tx.clone();

                        app_state = AppState::Connected {
                            my_name: my_name.clone(),
                            peer_addr: addr,
                            peer_name: peer_name.clone(),
                            tx: net_send_tx,
                        };
                        chat_history.push(format!("[System]: Connection authorized by @{}.", peer_name));

                        let (mut reader, mut writer) = stream.into_split();
                        tokio::spawn(async move {
                            let mut read_buf = [0u8; 512];
                            loop {
                                tokio::select! {
                                    res = reader.read_exact(&mut read_buf) => {
                                        match res {
                                            Ok(_) => {
                                                if read_buf[0] == 1 {
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
                    } else {
                        app_state = current_state;
                    }
                }
                AppEvent::NetworkMessage(msg) => {
                    if let AppState::Connected { peer_name, .. } = &app_state {
                        chat_history.push(format!("[{}]: {}", peer_name, msg));
                    }
                }
                AppEvent::NetworkDisconnected => {
                    let current_state = std::mem::replace(&mut app_state, AppState::InputName);
                    match current_state {
                        AppState::Connected { my_name, .. } | AppState::IncomingRequest { my_name, .. } => {
                            chat_history.push("[System]: Connection closed by peer.".to_string());
                            app_state = AppState::Listening { my_name };
                        }
                        other => {
                            app_state = other;
                        }
                    }
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