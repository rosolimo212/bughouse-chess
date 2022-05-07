// TODO: Rename to client_tui (or client_console).
// TODO: Allow all commands (including "next game" and others).

use std::fmt;
use std::io;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crossterm::{execute, terminal, cursor, event as term_event};
use crossterm::style::{self, Stylize};
use enum_map::{EnumMap, enum_map};
use instant::Instant;
use itertools::Itertools;
use scopeguard::defer;
use tungstenite::protocol;
use url::Url;

use bughouse_chess::*;
use bughouse_chess::client::*;

use crate::network;
use crate::tui;


pub struct ClientConfig {
    pub server_address: String,
    pub player_name: String,
    pub team: String,
}

enum IncomingEvent {
    Network(BughouseServerEvent),
    Terminal(term_event::Event),
    Tick,
}

fn writeln_raw(stdout: &mut io::Stdout, v: impl fmt::Display) -> io::Result<()> {
    let s = v.to_string();
    // Note. Not using `lines()` because it removes trailing new line.
    for line in s.split('\n') {
        execute!(stdout, style::Print(line), cursor::MoveToNextLine(1), cursor::Hide)?;
    }
    Ok(())
}

fn render(
    stdout: &mut io::Stdout, app_start_time: Instant, client_state: &ClientState,
    keyboard_input: &str, command_error: &Option<String>
)
    -> io::Result<()>
{
    let my_name = client_state.my_name();
    let now = Instant::now();
    execute!(stdout, cursor::MoveTo(0, 0))?;
    let mut highlight_input = false;
    let mut additional_message = None;
    match client_state.contest_state() {
        ContestState::Uninitialized => {
            execute!(stdout, terminal::Clear(terminal::ClearType::All))?;
            writeln_raw(stdout, "Loading...")?;
        },
        ContestState::Lobby{ players } => {
            execute!(stdout, terminal::Clear(terminal::ClearType::All))?;
            let mut teams: EnumMap<Team, Vec<String>> = enum_map!{ _ => vec![] };
            for p in players {
                teams[p.team].push(p.name.clone());
            }
            for (team, team_players) in teams {
                writeln_raw(stdout, &format!("Team {:?}:", team))?;
                let color = match team {
                    Team::Red => style::Color::Red,
                    Team::Blue => style::Color::Blue,
                };
                for p in team_players {
                    writeln_raw(stdout, format!("  {} {}", "•".with(color), p))?;
                }
                writeln_raw(stdout, "")?;
            }
        },
        ContestState::Game{ game_confirmed, local_turn, time_pair, .. } => {
            // TODO: Show scores
            let game_now = GameInstant::from_pair_game_maybe_active(*time_pair, now);
            let game = game_local(my_name, game_confirmed, local_turn);
            let view = game.view_for_player(my_name);
            writeln_raw(stdout, format!("{}\n", tui::render_bughouse_game(&game, view, game_now)))?;
            // Note. Don't clear the board to avoid blinking.
            // TODO: Show last turn by opponent.
            execute!(stdout, terminal::Clear(terminal::ClearType::FromCursorDown))?;
            if game.status() == BughouseGameStatus::Active {
                highlight_input = game.player_is_active(my_name).unwrap();
            } else {
                additional_message = Some(
                    format!("Game over: {:?}", game.status()).with(style::Color::Magenta)
                );
            }
        },
    }

    // Improvement potential. Fix: the bottom is blinking and input is lagging.
    // Simulate cursor: real cursor blinking is broken with Show/Hide.
    let show_cursor = now.duration_since(app_start_time).as_millis() % 1000 >= 500;
    let cursor = if show_cursor { '▂' } else { ' ' };
    let input_with_cursor = format!("{}{}", keyboard_input, cursor);
    let input_style = if highlight_input { style::Color::White } else { style::Color::DarkGrey };
    // Improvement potential. Show input on a fixed line regardless of client_status.
    writeln_raw(stdout, format!("{}\n", input_with_cursor.with(input_style)))?;

    if let Some(msg) = additional_message {
        writeln_raw(stdout, msg)?;
    }
    if let Some(ref err) = command_error {
        writeln_raw(stdout, err.clone().with(style::Color::Red))?;
    }
    Ok(())
}

pub fn run(config: ClientConfig) -> io::Result<()> {
    let my_name = config.player_name.trim().to_owned();
    let my_team = match config.team.as_str() {
        "red" => Team::Red,
        "blue" => Team::Blue,
        _ => panic!("Unexpected team: {}", config.team),
    };
    let server_addr = (config.server_address.as_str(), network::PORT).to_socket_addrs().unwrap().collect_vec();
    println!("Connecting to {:?}...", server_addr);
    let stream = TcpStream::connect(&server_addr[..])?;
    // Improvement potential: Test if nodelay helps. Should it be set on both sides or just one?
    //   net_stream.set_nodelay(true)?;
    let ws_request = Url::parse(&format!("ws://{}", config.server_address)).unwrap();
    let (mut socket_in, _) = tungstenite::client(ws_request, stream).unwrap();
    let mut socket_out = network::clone_websocket(&socket_in, protocol::Role::Client);
    std::mem::drop(config);

    let mut stdout = io::stdout();
    terminal::enable_raw_mode()?;  // TODO: Should this be reverted on exit?
    execute!(stdout, terminal::EnterAlternateScreen, cursor::Hide)?;
    defer!{ execute!(io::stdout(), terminal::LeaveAlternateScreen).unwrap(); };
    let app_start_time = Instant::now();

    let (tx, rx) = mpsc::channel();
    let tx_net = tx.clone();
    let tx_local = tx.clone();
    let tx_tick = tx;
    thread::spawn(move || {
        loop {
            let ev = network::read_obj(&mut socket_in).unwrap();
            tx_net.send(IncomingEvent::Network(ev)).unwrap();
        }
    });
    thread::spawn(move || {
        loop {
            let ev = term_event::read().unwrap();
            tx_local.send(IncomingEvent::Terminal(ev)).unwrap();
        }
    });
    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_millis(100));
            tx_tick.send(IncomingEvent::Tick).unwrap();
        }
    });

    let (server_tx, server_rx) = mpsc::channel();
    thread::spawn(move || {
        for ev in server_rx {
            network::write_obj(&mut socket_out, &ev).unwrap();
        }
    });

    let mut client_state = ClientState::new(my_name.to_owned(), my_team, server_tx);
    let mut keyboard_input = String::new();
    let mut command_error = None;
    client_state.join();
    for event in rx {
        match event {
            IncomingEvent::Network(event) => {
                match client_state.process_server_event(event).unwrap() {
                    NotableEvent::None => {},
                    NotableEvent::GameStarted => {
                        execute!(stdout, terminal::Clear(terminal::ClearType::All))?;
                    }
                    NotableEvent::OpponentTurnMade(_) => {},
                }
            },
            IncomingEvent::Terminal(event) => {
                if let term_event::Event::Key(event) = event {
                    match event.code {
                        term_event::KeyCode::Char(ch) => {
                            keyboard_input.push(ch);
                        },
                        term_event::KeyCode::Backspace => {
                            keyboard_input.pop();
                        },
                        term_event::KeyCode::Enter => {
                            let mut keep_input = false;
                            if let Some(cmd) = keyboard_input.strip_prefix('/') {
                                match cmd {
                                    "quit" => {
                                        client_state.leave();
                                        return Ok(());
                                    },
                                    "resign" => {
                                        client_state.resign();
                                    },
                                    _ => {
                                        command_error = Some(format!("Unknown command: '{}'", cmd));
                                    },
                                }
                            } else {
                                command_error = match client_state.make_turn(keyboard_input.clone()) {
                                    Ok(()) => None,
                                    Err(TurnCommandError::IllegalTurn(TurnError::WrongTurnOrder)) => {
                                        keep_input = true;
                                        None
                                    },
                                    Err(TurnCommandError::IllegalTurn(err)) => {
                                        Some(format!("Illegal turn '{}': {:?}", keyboard_input, err))
                                    },
                                    Err(TurnCommandError::NoGameInProgress) => {
                                        Some("Cannot make turn: no game in progress".to_owned())
                                    },
                                }
                            }
                            if !keep_input {
                                keyboard_input.clear();
                            }
                        },
                        _ => {},
                    }
                }
            },
            IncomingEvent::Tick => {
                // Any event triggers repaint, so no additional action is required.
            },
        }
        render(&mut stdout, app_start_time, &client_state, &keyboard_input, &command_error)?;
    }
    panic!("Unexpected end of events stream");
}
