// Improvement potential: Add analog of bughouse_online.rs test. Connect multiple clients
// to a virtual server and execute random actions on the clients. Verify that the server
// and the clients do not panic.

use std::cell::RefCell;
use std::{io, panic};

use instant::Instant;
use rand::prelude::*;
use rand::distributions::WeightedIndex;
use rand::seq::SliceRandom;

use bughouse_chess::*;
use bughouse_chess::test_util::*;


const GAMES_PER_BATCH: usize = 100;
const TURNS_PER_GAME: usize = 100_000;
const ACTIONS_PER_GAME: usize = 10_000;
const MAX_ATTEMPTS_GENERATING_SERVER_TURN: usize = 10_000;
const DROP_RATIO: f64 = 0.2;
const DRAG_RESERVE_RATIO: f64 = 0.3;
const DRAG_OVER_BOARD_RATIO: f64 = 0.8;
const PROMOTION_RATIO: f64 = 0.2;
const QUIT_INACTIVE_GAME_RATIO: f64 = 0.1;

pub struct StressTestConfig {
    pub target: String,
}

#[derive(Clone, Copy, Debug)]
enum ActionKind {
    SetStatus,
    ApplyRemoteTurn,
    LocalTurn,
    PieceDragState,
    StartDragPiece,
    DragOverPiece,
    AbortDragPiece,
    DragPieceDrop,
    CancelPreturn,
}

// Improvement potential. Find or implement a way to automatically generate such a enum and apply it.
#[derive(Clone, Debug)]
enum Action {
    SetStatus{ status: BughouseGameStatus, time: GameInstant },
    ApplyRemoteTurn{ player_id: BughousePlayerId, turn_algebraic: String, time: GameInstant },
    LocalTurn{ turn_input: TurnInput, time: GameInstant },
    PieceDragState,
    StartDragPiece{ start: PieceDragStart },
    DragOverPiece{ dest: Option<Coord> },
    AbortDragPiece,
    DragPieceDrop{ dest: Coord, promote_to: PieceKind },
    CancelPreturn,
}

#[derive(Default)]
struct TestState {
    alt_game: Option<AlteredGame>,
    last_action: Option<Action>,
}

thread_local! {
    static TEST_STATE: RefCell<TestState> = RefCell::new(TestState::default());
}

fn default_bughouse_game() -> BughouseGame {
    BughouseGame::new(ChessRules::classic_blitz(), BughouseRules::chess_com(), &sample_bughouse_players())
}

fn random_coord(rng: &mut rand::rngs::ThreadRng) -> Coord {
    Coord::new(
        Row::from_zero_based(rng.gen_range(0..7)),
        Col::from_zero_based(rng.gen_range(0..7))
    )
}

fn random_piece(rng: &mut rand::rngs::ThreadRng) -> PieceKind {
    use PieceKind::*;
    let pieces = [ Pawn, Knight, Bishop, Rook, Queen, King ];
    *pieces.choose(rng).unwrap()
}

fn random_force(rng: &mut rand::rngs::ThreadRng) -> Force {
    if rng.gen::<bool>() { Force::White } else { Force::Black }
}

fn random_board(rng: &mut rand::rngs::ThreadRng) -> BughouseBoard {
    if rng.gen::<bool>() { BughouseBoard::A } else { BughouseBoard::B }
}

fn random_turn(rng: &mut rand::rngs::ThreadRng) -> Turn {
    // Note: Castling is also covered thanks to `TurnInput::DragDrop`.
    if rng.gen_bool(DROP_RATIO) {
        Turn::Drop(TurnDrop {
            to: random_coord(rng),
            piece_kind: random_piece(rng),
        })
    } else {
        // Trying to strike balance: on the one hand, we want to include invalid turns.
        // On the other hand, too many invalid turns would mean that very little happens.
        // Decision: randomly try to promote all pieces (pawns and non-pawns), but only
        // if they are potentially on the last row.
        let from = random_coord(rng);
        let to = random_coord(rng);
        let promote_to = if to.row == Row::_1 || to.row == Row::_8 && rng.gen_bool(PROMOTION_RATIO) {
            Some(random_piece(rng))
        } else {
            None
        };
        Turn::Move(TurnMove{ from, to, promote_to })
    }
}

fn random_action_kind(rng: &mut rand::rngs::ThreadRng) -> ActionKind {
    let n = 10_000_000;
    assert!(n >= TURNS_PER_GAME * 10);
    use ActionKind::*;
    let weighted_actions = [
        (SetStatus, n / TURNS_PER_GAME / 2),  // these end the game, so should only have very few
        (ApplyRemoteTurn, n / 10),  // these are always valid (and expensive)
        (LocalTurn, n),
        (PieceDragState, n),
        (StartDragPiece, n),
        (DragOverPiece, n),
        (AbortDragPiece, n),
        (DragPieceDrop, n),
        (CancelPreturn, n),
    ];
    let (actions, weights): (Vec<_>, Vec<_>) = weighted_actions.into_iter().unzip();
    let dist = WeightedIndex::new(&weights).unwrap();
    actions[dist.sample(rng)]
}

fn random_action(alt_game: &AlteredGame, rng: &mut rand::rngs::ThreadRng) -> Option<Action> {
    use ActionKind::*;
    Some(match random_action_kind(rng) {
        SetStatus => {
            let status = BughouseGameStatus::Victory(Team::Red, VictoryReason::Resignation);
            let time = GameInstant::game_start();
            Action::SetStatus{ status, time }
        },
        ApplyRemoteTurn => {
            // Optimization potential. A more direct way of generating random valid moves.
            for _ in 0..MAX_ATTEMPTS_GENERATING_SERVER_TURN {
                let mut game = alt_game.game_confirmed().clone();
                let mut board_idx = random_board(rng);
                let mut force = game.board(board_idx).active_force();
                let mut player_id = BughousePlayerId{ board_idx, force };
                // The client can reasonably expect that the server wouldn't send back turns by
                // the current player other than those which they actually made (except while
                // reconnecting). Some assertions along those lines are sprinkled here and there
                // in AlteredGame. But we don't want to track too much state in this test. This
                // would be a step away from fuzzing towards traditional integration testing,
                // which is not the focus here. So here's a, ahem, solution: never confirm any
                // turns by the current player! This limits the scope of the test, of course,
                // but I don't have better ideas for now. Perhaps AlteredGame is just a bad
                // layer of abstraction for fuzzing, and this test should be deleted when a
                // client/server fuzzer test is in place.
                if alt_game.my_id() == BughouseParticipantId::Player(player_id) {
                    board_idx = board_idx.other();
                    force = game.board(board_idx).active_force();
                    player_id = BughousePlayerId{ board_idx, force };
                }
                let turn = random_turn(rng);
                let turn_is_valid = game.try_turn(
                    board_idx,
                    &TurnInput::DragDrop(turn),
                    TurnMode::Normal,
                    GameInstant::game_start()
                ).is_ok();
                if turn_is_valid {
                    let turn_algebraic = game.last_turn_record().unwrap().turn_expanded.algebraic.clone();
                    let time = GameInstant::game_start();
                    return Some(Action::ApplyRemoteTurn{ player_id, turn_algebraic, time });
                }
            }
            return None;
        },
        LocalTurn => {
            let turn_input = TurnInput::DragDrop(random_turn(rng));
            let time = GameInstant::game_start();
            Action::LocalTurn{ turn_input, time }
        },
        PieceDragState => {
            Action::PieceDragState
        },
        StartDragPiece => {
            let start = if rng.gen_bool(DRAG_RESERVE_RATIO) {
                PieceDragStart::Reserve(random_piece(rng))
            } else {
                PieceDragStart::Board(random_coord(rng))
            };
            Action::StartDragPiece{ start }
        },
        DragOverPiece => {
            let dest = if rng.gen_bool(DRAG_OVER_BOARD_RATIO) {
                Some(random_coord(rng))
            } else {
                None
            };
            Action::DragOverPiece{ dest }
        },
        AbortDragPiece => {
            Action::AbortDragPiece
        },
        DragPieceDrop => {
            let dest = random_coord(rng);
            let promote_to = PieceKind::Queen;
            Action::DragPieceDrop{ dest, promote_to }
        },
        CancelPreturn => {
            Action::CancelPreturn
        },
    })
}

fn apply_action(alt_game: &mut AlteredGame, action: Action) {
    use Action::*;
    match action {
        SetStatus{ status, time } => _ = alt_game.set_status(status, time),
        ApplyRemoteTurn{ player_id, turn_algebraic, time } =>
            _ = alt_game.apply_remote_turn_algebraic(player_id, &turn_algebraic, time),
        LocalTurn{ turn_input, time } => _ = alt_game.try_local_turn(turn_input, time),
        PieceDragState => _ = alt_game.piece_drag_state(),
        StartDragPiece{ start } => _ = alt_game.start_drag_piece(start),
        DragOverPiece{ dest } => _ = alt_game.drag_over_piece(dest),
        AbortDragPiece => _ = alt_game.abort_drag_piece(),
        DragPieceDrop{ dest, promote_to } => _ = alt_game.drag_piece_drop(dest, promote_to),
        CancelPreturn => _ = alt_game.cancel_preturn(),
    }
}


pub fn bughouse_game_test() -> io::Result<()> {
    let rng = &mut rand::thread_rng();
    loop {
        let t0 = Instant::now();
        let mut finished_games = 0;
        let mut total_turns = 0;
        let mut successful_turns = 0;
        for _ in 0..GAMES_PER_BATCH {
            let mut game = default_bughouse_game();
            for _ in 0..TURNS_PER_GAME {
                let ret = game.try_turn(
                    random_board(rng),
                    &TurnInput::DragDrop(random_turn(rng)),
                    TurnMode::Normal,
                    GameInstant::game_start()
                );
                total_turns += 1;
                if ret.is_ok() {
                    successful_turns += 1;
                }
                if game.status() != BughouseGameStatus::Active && rng.gen_bool(QUIT_INACTIVE_GAME_RATIO) {
                    break;
                }
            }
            if game.status() != BughouseGameStatus::Active {
                finished_games += 1;
            }
        }
        let elpased = t0.elapsed();
        println!(
            "Ran: {} games ({} finished), {} turns ({} successful) in {:.2}s",
            GAMES_PER_BATCH,
            finished_games,
            total_turns,
            successful_turns,
            elpased.as_secs_f64(),
        );
    }
}

pub fn altered_game_test() -> io::Result<()> {
    let std_panic_hook = panic::take_hook();
    panic::set_hook(Box::new(move |panic_info| {
        TEST_STATE.with(|cell| {
            if let Some(ref last_action) = cell.borrow().last_action {
                println!("Last action: {last_action:?}");
            }
            if let Some(ref alt_game) = cell.borrow().alt_game {
               println!("AlteredGame before action:\n{alt_game:#?}");
            }
        });
        std_panic_hook(panic_info);
    }));
    let rng = &mut rand::thread_rng();
    loop {
        let t0 = Instant::now();
        let mut finished_games = 0;
        for _ in 0..GAMES_PER_BATCH {
            let participant = BughouseParticipantId::Player(BughousePlayerId {
                board_idx: random_board(rng),
                force: random_force(rng),
            });
            let mut alt_game = AlteredGame::new(participant, default_bughouse_game());
            for _ in 0..ACTIONS_PER_GAME {
                let Some(action) = random_action(&alt_game, rng) else {
                    break;
                };
                TEST_STATE.with(|cell| {
                    let state = &mut cell.borrow_mut();
                    state.alt_game = Some(alt_game.clone());
                    state.last_action = Some(action.clone());
                });
                apply_action(&mut alt_game, action);
                alt_game.local_game();
                if alt_game.status() != BughouseGameStatus::Active && rng.gen_bool(QUIT_INACTIVE_GAME_RATIO) {
                    break;
                }
            }
            if alt_game.status() != BughouseGameStatus::Active {
                finished_games += 1;
            }
        }
        let elpased = t0.elapsed();
        println!(
            "Ran: {} games ({} finished) in {:.2}s",
            GAMES_PER_BATCH,
            finished_games,
            elpased.as_secs_f64(),
        );
    }
}

pub fn run(config: StressTestConfig) -> io::Result<()> {
    match config.target.as_str() {
        "pure-game" => bughouse_game_test(),
        "altered-game" => altered_game_test(),
        _ => panic!("Invalid stress test target: {}", config.target),
    }
}
