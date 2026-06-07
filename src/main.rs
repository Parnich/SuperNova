use crossterm::{
    event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures::StreamExt;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    widgets::{Paragraph, Wrap},
    Terminal,
};
use std::{io, net::SocketAddr};
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
    },
}

enum AppEvent {
    Terminal(KeyEvent),
    NetworkNewConnection(tokio::net::TcpStream, SocketAddr),
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
                AppState::Connected { peer_name, peer_addr } => format!("Connected with @{} ({})", peer_name, peer_addr),
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
                                    chat_history.push(format!("[You]: {}", input_text));
                                    input_text.clear();
                                }
                            }
                            _ => {}
                        }
                    }
                }
                AppEvent::NetworkNewConnection(stream, addr) => {
                    match app_state {
                        AppState::Listening => {
                            app_state = AppState::Connected {
                                peer_addr: addr,
                                peer_name: "Unknown_Peer".to_string(),
                            };
                            chat_history.push(format!("[System]: Peer [{}] detected. Establishing encrypted session...", addr));

                            tokio::spawn(async move {
                                let _sh = stream;
                            });
                        }
                        AppState::Connected { .. } => {
                            drop(stream);
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