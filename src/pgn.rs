use std::rc::Rc;

use enum_map::enum_map;
use itertools::Itertools;
use serde::{Serialize, Deserialize};
use strum::IntoEnumIterator;
use time::macros::format_description;

use crate::board::{Board, VictoryReason, DrawReason};
use crate::clock::TimeControl;
use crate::fen;
use crate::force::Force;
use crate::game::{TurnRecordExpanded, BughousePlayerId, BughouseBoard, BughouseGameStatus, BughouseGame, get_bughouse_force};
use crate::player::{Team, PlayerInGame};
use crate::rules::StartingPosition;


#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct BughouseExportFormat {
    // TODO: Support time export and allow to choose format here:
    //   - as described in https://bughousedb.com/Lieven_BPGN_Standard.txt, but export
    //       with milliseconds precision, like https://bughousedb.com itself does.
    //   - as in chess.com, e.g. "{[%clk 0:00:07.9]}"
    //   - precise
    //   - none
}

const LINE_WIDTH: usize = 80;

struct TextDocument {
    text: String,
    last_line_len: usize,
}
impl TextDocument {
    fn new() -> Self { TextDocument{ text: String::new(), last_line_len: 0 } }
    fn push_word(&mut self, word: &str) {
        const SPACE_WIDTH: usize = 1;
        if self.last_line_len == 0 {
            // no separators: first record
        } else if self.last_line_len + word.len() + SPACE_WIDTH <= LINE_WIDTH {
            self.text.push(' ');
            self.last_line_len += SPACE_WIDTH;
        } else {
            self.text.push('\n');
            self.last_line_len = 0;
        }
        self.text.push_str(word);
        self.last_line_len += word.len();
    }
    fn render(&self) -> String {
        let trailing_newline = if self.last_line_len > 0 { "\n" } else { "" };
        format!("{}{}", self.text, trailing_newline)
    }
}

fn time_control_to_string(control: &TimeControl) -> String {
    control.starting_time.as_secs().to_string()
}

fn make_result_string(game: &BughouseGame) -> &'static str {
    use BughouseGameStatus::*;
    match game.status() {
        Active => "*",
        Draw(_) => "1/2-1/2",
        Victory(team, _) => match team {
            Team::Red => "1-0",
            Team::Blue => "0-1",
        },
    }
}

fn make_termination_string(game: &BughouseGame) -> String {
    use BughouseGameStatus::*;
    use VictoryReason::*;
    use DrawReason::*;
    let make_team_string = |team| {
        // Note. Not using `game.players()` because the order there is not specified.
        BughouseBoard::iter()
            .map(|board_idx| &game.board(board_idx).player(get_bughouse_force(team, board_idx)).name)
            .join(" & ")
    };
    match game.status() {
        Active => "Unterminated".to_owned(),
        Victory(team, Checkmate) => format!("{} won by checkmate", make_team_string(team)),
        Victory(team, Flag) => format!("{} won by flag", make_team_string(team)),
        Victory(team, Resignation) => format!("{} won by resignation", make_team_string(team)),
        Draw(SimultaneousFlag) => "Draw by simultaneous flags".to_owned(),
        Draw(ThreefoldRepetition) => "Draw by threefold repetition".to_owned(),
    }
}

// Dummy player object. Will not appear anywhere in the produced PGN.
fn dummy_player(team: Team) -> Rc<PlayerInGame> {
    Rc::new(PlayerInGame{ name: format!("{:?}", team), team })
}

// Improvement potential. More human-readable "Termination" tag values.
fn make_bughouse_bpng_header(game: &BughouseGame, round: usize) -> String {
    use BughouseBoard::*;
    use Force::*;
    let now = time::OffsetDateTime::now_utc();
    let (variant, starting_position_fen) = match game.chess_rules().starting_position {
        StartingPosition::Classic =>
            ("Bughouse", String::new()),
        StartingPosition::FischerRandom => {
            let starting_board = Board::new(
                Rc::clone(game.chess_rules()),
                Some(Rc::clone(game.bughouse_rules())),
                enum_map!{ White => dummy_player(Team::Red), Black => dummy_player(Team::Blue) },
                game.starting_position()
            );
            let one_board = fen::starting_position_to_shredder_fen(&starting_board);
            ("Bughouse Chess960", format!("[FEN \"{one_board} | {one_board}\"]\n"))
        }
    };
    // TODO: Save complete rules.
    format!(
r#"[Event "Friendly Bughouse Match"]
[Site "bughouse.pro"]
[UTCDate "{}"]
[UTCTime "{}"]
[Round "{}"]
[WhiteA "{}"]
[BlackA "{}"]
[WhiteB "{}"]
[BlackB "{}"]
[TimeControl "{}"]
[Variant "{}"]
[SetUp "1"]
{}[Result "{}"]
[Termination "{}"]
"#,
        now.format(format_description!("[year].[month].[day]")).unwrap(),
        now.format(format_description!("[hour]:[minute]:[second]")).unwrap(),
        round,
        game.board(A).player(White).name,
        game.board(A).player(Black).name,
        game.board(B).player(White).name,
        game.board(B).player(Black).name,
        time_control_to_string(&game.chess_rules().time_control),
        variant,
        starting_position_fen,
        make_result_string(game),
        make_termination_string(game),
    )
}

fn player_notation(player_id: BughousePlayerId) -> &'static str {
    use BughouseBoard::*;
    use Force::*;
    match (player_id.board_idx, player_id.force) {
        (A, White) => "A",
        (A, Black) => "a",
        (B, White) => "B",
        (B, Black) => "b",
    }
}

// Exports to BPGN (Bughouse Portable Game Notation) - format designed specifically for
// bughouse. Doc: https://bughousedb.com/Lieven_BPGN_Standard.txt
// Based on PGN (Portable Game Notation), the de-facto standard plain format for recording
// chess games. Doc: http://www.saremba.de/chessgml/standards/pgn/pgn-complete.htm
pub fn export_to_bpgn(_format: BughouseExportFormat, game: &BughouseGame, round: usize)
    -> String
{
    let header = make_bughouse_bpng_header(game, round);
    let mut doc = TextDocument::new();
    let mut full_turn_idx = enum_map!{ _ => 1 };
    for turn_record in game.turn_log() {
        let TurnRecordExpanded{ player_id, turn_expanded, .. } = turn_record;
        let turn_algebraic = &turn_expanded.algebraic;
        let turn_notation = format!(
            "{}{}. {}",
            full_turn_idx[player_id.board_idx],
            player_notation(*player_id),
            turn_algebraic,
        );
        if player_id.force == Force::Black {
            full_turn_idx[player_id.board_idx] += 1;
        }
        doc.push_word(&turn_notation);
    }
    format!("{}{}", header, doc.render())
}
