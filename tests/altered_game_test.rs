
mod common;

use bughouse_chess::*;
use bughouse_chess::test_util::*;
use common::*;
use BughouseBoard::A;


fn as_player(player_id: BughousePlayerId) -> BughouseParticipantId {
    BughouseParticipantId::Player(player_id)
}

fn default_bughouse_game() -> BughouseGame {
    BughouseGame::new(ChessRules::classic_blitz(), BughouseRules::chess_com(), &sample_bughouse_players())
}

const GAME_START: GameInstant = GameInstant::game_start();


// Regression test: shouldn't panic if there's a drag depending on a local turn that was reverted.
#[test]
fn drag_depends_on_reverted_preturn() {
    let mut alt_game = AlteredGame::new(as_player(seating!(Black A)), default_bughouse_game());
    alt_game.apply_remote_turn_algebraic(seating!(White A), "e4", GAME_START).unwrap();
    alt_game.apply_remote_turn_algebraic(seating!(Black A), "e6", GAME_START).unwrap();
    alt_game.try_local_turn(drag_move!(E6 -> E5), GAME_START).unwrap();
    alt_game.start_drag_piece(PieceDragStart::Board(Coord::E5)).unwrap();
    alt_game.apply_remote_turn_algebraic(seating!(White A), "e5", GAME_START).unwrap();
    assert_eq!(alt_game.drag_piece_drop(Coord::E4, PieceKind::Queen), Err(PieceDragError::DragNoLongerPossible));
}

// It is not allowed to have more than one preturn. However a player can start dragging a
// piece while having a preturn and finish the drag after the preturn was upgraded to a
// regular local turn (or resolved altogether).
#[test]
fn start_drag_with_a_preturn() {
    let mut alt_game = AlteredGame::new(as_player(seating!(White A)), default_bughouse_game());
    alt_game.try_local_turn(drag_move!(E2 -> E3), GAME_START).unwrap();
    alt_game.try_local_turn(drag_move!(E3 -> E4), GAME_START).unwrap();
    alt_game.start_drag_piece(PieceDragStart::Board(Coord::E4)).unwrap();
    alt_game.apply_remote_turn_algebraic(seating!(White A), "e3", GAME_START).unwrap();
    alt_game.apply_remote_turn_algebraic(seating!(Black A), "Nc6", GAME_START).unwrap();
    let drag_result = alt_game.drag_piece_drop(Coord::E5, PieceKind::Queen).unwrap();
    assert_eq!(drag_result, drag_move!(E4 -> E5));
}

// Regression test: keep local preturn after getting an opponent's turn.
// Original implementation missed it because it expected that the server always sends our
// preturn back together with the opponent's turn. And it does. When it *has* the preturn.
// But with the preturn still in-flight, a race condition happened.
#[test]
fn pure_preturn_persistent() {
    let mut alt_game = AlteredGame::new(as_player(seating!(Black A)), default_bughouse_game());
    alt_game.try_local_turn(algebraic_turn("e5"), GAME_START).unwrap();
    alt_game.apply_remote_turn_algebraic(seating!(White A), "e4", GAME_START).unwrap();
    assert!(alt_game.local_game().board(A).grid()[Coord::E5].is(piece!(Black Pawn)));
}

#[test]
fn preturn_invalidated() {
    let mut alt_game = AlteredGame::new(as_player(seating!(White A)), default_bughouse_game());
    alt_game.apply_remote_turn_algebraic(seating!(White A), "e4", GAME_START).unwrap();
    alt_game.try_local_turn(algebraic_turn("e5"), GAME_START).unwrap();
    assert!(alt_game.local_game().board(A).grid()[Coord::E5].is(piece!(White Pawn)));

    alt_game.apply_remote_turn_algebraic(seating!(Black A), "e5", GAME_START).unwrap();
    assert!(alt_game.local_game().board(A).grid()[Coord::E5].is(piece!(Black Pawn)));
}

#[test]
fn preturn_after_local_turn_persistent() {
    let mut alt_game = AlteredGame::new(as_player(seating!(White A)), default_bughouse_game());
    alt_game.try_local_turn(algebraic_turn("e4"), GAME_START).unwrap();
    alt_game.try_local_turn(algebraic_turn("e5"), GAME_START).unwrap();
    assert!(alt_game.local_game().board(A).grid()[Coord::E5].is(piece!(White Pawn)));

    alt_game.apply_remote_turn_algebraic(seating!(White A), "e4", GAME_START).unwrap();
    assert!(alt_game.local_game().board(A).grid()[Coord::E5].is(piece!(White Pawn)));

    alt_game.apply_remote_turn_algebraic(seating!(Black A), "Nc6", GAME_START).unwrap();
    assert!(alt_game.local_game().board(A).grid()[Coord::E5].is(piece!(White Pawn)));
}

#[test]
fn two_preturns_forbidden() {
    let mut alt_game = AlteredGame::new(as_player(seating!(White A)), default_bughouse_game());
    alt_game.try_local_turn(drag_move!(E2 -> E4), GAME_START).unwrap();
    alt_game.try_local_turn(drag_move!(D2 -> D4), GAME_START).unwrap();
    assert_eq!(alt_game.try_local_turn(drag_move!(F2 -> F4), GAME_START), Err(TurnError::PreturnLimitReached));
}
