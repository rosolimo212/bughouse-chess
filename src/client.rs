use std::rc::Rc;
use std::sync::mpsc;

use enum_map::EnumMap;
use instant::Instant;

use crate::altered_game::AlteredGame;
use crate::board::TurnError;
use crate::clock::{GameInstant, WallGameTimePair};
use crate::game::{BughouseGameStatus, BughouseGame};
use crate::event::{TurnRecord, BughouseServerEvent, BughouseClientEvent};
use crate::player::{Player, Team};
use crate::util::try_vec_to_enum_map;


#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TurnCommandError {
    IllegalTurn(TurnError),
    NoGameInProgress,
}

#[derive(Clone, Debug)]
pub enum NotableEvent {
    None,
    GameStarted,
    OpponentTurnMade,
}

#[derive(Clone, Debug)]
pub enum EventError {
    ServerReturnedError(String),
    CannotApplyEvent(String),
}

#[derive(Debug)]
pub enum ContestState {
    Uninitialized,
    Lobby { players: Vec<Player> },
    Game {
        // Scores from the past matches.
        scores: EnumMap<Team, u32>,
        // Game state including unconfirmed local changes.
        alt_game: AlteredGame,
        // Game start time: `None` before first move, non-`None` afterwards.
        time_pair: Option<WallGameTimePair>,
    },
}

pub struct ClientState {
    my_name: String,
    my_team: Team,
    events_tx: mpsc::Sender<BughouseClientEvent>,
    contest_state: ContestState,
}

impl ClientState {
    pub fn new(my_name: String, my_team: Team, events_tx: mpsc::Sender<BughouseClientEvent>) -> Self {
        ClientState {
            my_name,
            my_team,
            events_tx,
            contest_state: ContestState::Uninitialized,
        }
    }

    pub fn my_name(&self) -> &str { &self.my_name }
    pub fn my_team(&self) -> Team { self.my_team }
    pub fn contest_state(&self) -> &ContestState { &self.contest_state }
    pub fn contest_state_mut(&mut self) -> &mut ContestState { &mut self.contest_state }

    pub fn join(&mut self) {
        self.events_tx.send(BughouseClientEvent::Join {
            player_name: self.my_name.to_owned(),
            team: self.my_team,
        }).unwrap();
    }
    pub fn resign(&mut self) {
        self.events_tx.send(BughouseClientEvent::Resign).unwrap();
    }
    pub fn next_game(&mut self) {
        self.events_tx.send(BughouseClientEvent::NextGame).unwrap();
    }
    pub fn leave(&mut self) {
        self.events_tx.send(BughouseClientEvent::Leave).unwrap();
    }
    pub fn reset(&mut self) {
        self.events_tx.send(BughouseClientEvent::Reset).unwrap();
    }

    pub fn make_turn(&mut self, turn_algebraic: String) -> Result<(), TurnCommandError> {
        if let ContestState::Game{ ref mut alt_game, time_pair, .. }
            = self.contest_state
        {
            let game_now = GameInstant::from_pair_game_maybe_active(time_pair, Instant::now());
            if alt_game.status() != BughouseGameStatus::Active {
                Err(TurnCommandError::IllegalTurn(TurnError::GameOver))
            } else if alt_game.can_make_local_turn() {
                alt_game.try_local_turn_algebraic(&turn_algebraic, game_now).map_err(|err| {
                    TurnCommandError::IllegalTurn(err)
                })?;
                self.events_tx.send(BughouseClientEvent::MakeTurn {
                    turn_algebraic: turn_algebraic
                }).unwrap();
                Ok(())
            } else {
                Err(TurnCommandError::IllegalTurn(TurnError::WrongTurnOrder))
            }
        } else {
            Err(TurnCommandError::NoGameInProgress)
        }
    }

    // TODO: This is becoming a weird mixture of rendering `ContestState` AND processing `NotableEvent`s.
    //   Consider whether `ClientState` should become a processor of turning events from server
    //   into more digestable client events that client implementations work on (while never reading
    //   the state directly.
    pub fn process_server_event(&mut self, event: BughouseServerEvent) -> Result<NotableEvent, EventError> {
        use BughouseServerEvent::*;
        match event {
            Error{ message } => {
                Err(EventError::ServerReturnedError(format!("Got error from server: {}", message)))
            },
            LobbyUpdated{ players } => {
                let new_players = players;
                match self.contest_state {
                    ContestState::Lobby{ ref mut players } => {
                        *players = new_players;
                    },
                    _ => {
                        self.new_contest_state(ContestState::Lobby {
                            players: new_players
                        });
                    },
                }
                Ok(NotableEvent::None)
            },
            GameStarted{ chess_rules, bughouse_rules, scores, starting_grid, players, time, turn_log } => {
                let player_map = BughouseGame::make_player_map(
                    players.iter().map(|(p, board_idx)| (Rc::new(p.clone()), *board_idx))
                );
                let time_pair = if turn_log.is_empty() {
                    assert!(time.elapsed_since_start().is_zero());
                    None
                } else {
                    Some(WallGameTimePair::new(Instant::now(), time.approximate()))
                };
                let alt_game = AlteredGame::new(
                    self.my_name.clone(),
                    BughouseGame::new_with_grid(
                        chess_rules, bughouse_rules, starting_grid, player_map
                    )
                );
                self.new_contest_state(ContestState::Game {
                    scores: try_vec_to_enum_map(scores).unwrap(),
                    alt_game,
                    time_pair,
                });
                for turn in turn_log {
                    self.apply_remote_turn(turn)?;
                }
                Ok(NotableEvent::GameStarted)
            },
            TurnsMade(event) => {
                let mut opponent_turn_made = false;
                for turn in event {
                    let is_opponent_turn = self.apply_remote_turn(turn)?;
                    opponent_turn_made |= is_opponent_turn;
                }
                Ok(if opponent_turn_made { NotableEvent::OpponentTurnMade } else { NotableEvent::None })
            },
            GameOver{ time, game_status, scores: new_scores } => {
                if let ContestState::Game{ ref mut alt_game, ref mut scores, .. }
                    = self.contest_state
                {
                    alt_game.reset_local_changes();
                    assert!(alt_game.status() == BughouseGameStatus::Active);
                    assert!(game_status != BughouseGameStatus::Active);
                    alt_game.set_status(game_status, time);
                    *scores = try_vec_to_enum_map(new_scores).unwrap();
                    Ok(NotableEvent::None)
                } else {
                    Err(EventError::CannotApplyEvent("Cannot record game result: no game in progress".to_owned()))
                }
            },
        }
    }

    // Returns if the turn was mady by current player opponent.
    fn apply_remote_turn(&mut self, event: TurnRecord) -> Result<bool, EventError> {
        let TurnRecord{ player_name, turn_algebraic, time, game_status, scores: new_scores } = event;
        if let ContestState::Game{
            ref mut alt_game, ref mut time_pair, ref mut scores, ..
        } = self.contest_state {
            if alt_game.status() != BughouseGameStatus::Active {
                return Err(EventError::CannotApplyEvent(format!("Cannot make turn {}: game over", turn_algebraic)));
            }
            if time_pair.is_none() {
                // Improvement potential. Sync client/server times better; consider NTP.
                let game_start = GameInstant::game_start().approximate();
                *time_pair = Some(WallGameTimePair::new(Instant::now(), game_start));
            }
            alt_game.apply_remote_turn_algebraic(
                &player_name, &turn_algebraic, time
            ).map_err(|err| {
                EventError::CannotApplyEvent(format!("Impossible turn: {}, error: {:?}", turn_algebraic, err))
            })?;
            if game_status != alt_game.status() {
                return Err(EventError::CannotApplyEvent(format!(
                    "Expected game status = {:?}, actual = {:?}", game_status, alt_game.status()
                )));
            }
            *scores = try_vec_to_enum_map(new_scores).unwrap();
            Ok(alt_game.are_opponents(&player_name, &self.my_name).unwrap())
        } else {
            Err(EventError::CannotApplyEvent("Cannot make turn: no game in progress".to_owned()))
        }
    }

    // TODO: Is this function needed? (maybe always produce a NotableEvent here)
    fn new_contest_state(&mut self, contest_state: ContestState) {
        self.contest_state = contest_state;
    }
}
