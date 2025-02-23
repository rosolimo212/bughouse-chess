// Improvement potential: Standardize naming.
// Improvement potential: Add tests verifying inverse and commutative relations.

use std::ops;

use serde::{Serialize, Deserialize};
use strum::EnumIter;

use crate::coord::{Row, Col, Coord, NUM_ROWS, NUM_COLS};
use crate::force::Force;
use crate::game::{BughouseBoard, BughouseParticipantId};


#[derive(Clone, Copy, PartialEq, Eq, Debug, EnumIter)]
pub enum DisplayBoard {
    Primary,
    Secondary,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, EnumIter)]
pub enum DisplayPlayer {
    Top,
    Bottom,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Perspective {
    PlayAsWhite,
    PlayAsBlack,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BoardOrientation {
    Normal,   // White at bottom
    Rotated,  // Black at bottom
}

// These coords describe board squares, like `Coord`. Both `x` and `y` are integers
// between 0 and 7. But here row 0 corresponds to the top-most row, which could be
// row '1' or row '8' on the board.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DisplayCoord {
    pub x: u8,
    pub y: u8,
}

// Floating-point coords associated with `Coord` coordinate system.
// Point (0., 0.) corresponds to the outer corner of 'a1' square, while
// point (8., 8.) corresponds to the outer corner of 'h8' square.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct FCoord {
    pub x: f64,
    pub y: f64,
}

// Floating-point coords associated with `DisplayCoord` coordinate system.
// Point (0., 0.) corresponds to the top left corner of the top left square, while
// point (8., 8.) corresponds to the bottom right corner of the bottom right square.
#[derive(Clone, Copy, Debug)]
pub struct DisplayFCoord {
    pub x: f64,
    pub y: f64,
}


impl Perspective {
    pub fn for_force(force: Force) -> Self {
        match force {
            Force::White => Perspective::PlayAsWhite,
            Force::Black => Perspective::PlayAsBlack,
        }
    }
}

pub fn get_board_index(board: DisplayBoard, viewer: BughouseParticipantId) -> BughouseBoard {
    match board {
        DisplayBoard::Primary => viewer.visual_board_idx(),
        DisplayBoard::Secondary => viewer.visual_board_idx().other(),
    }
}

pub fn get_display_board_index(board: BughouseBoard, viewer: BughouseParticipantId) -> DisplayBoard {
    if viewer.visual_board_idx() == board { DisplayBoard::Primary } else { DisplayBoard::Secondary }
}

pub fn get_board_orientation(board: DisplayBoard, perspective: Perspective) -> BoardOrientation {
    use DisplayBoard::*;
    use Perspective::*;
    match (board, perspective) {
        (Primary, PlayAsWhite) | (Secondary, PlayAsBlack) => BoardOrientation::Normal,
        (Primary, PlayAsBlack) | (Secondary, PlayAsWhite) => BoardOrientation::Rotated,
    }
}

pub fn get_display_player(force: Force, orientation: BoardOrientation) -> DisplayPlayer {
    use Force::*;
    use BoardOrientation::*;
    match (orientation, force) {
        (Normal, White) | (Rotated, Black) => DisplayPlayer::Bottom,
        (Normal, Black) | (Rotated, White) => DisplayPlayer::Top,
    }
}

pub fn to_display_coord(coord: Coord, orientation: BoardOrientation) -> DisplayCoord {
    match orientation {
        BoardOrientation::Normal => DisplayCoord {
            x: coord.col.to_zero_based(),
            y: NUM_ROWS - coord.row.to_zero_based() - 1,
        },
        BoardOrientation::Rotated => DisplayCoord {
            x: NUM_COLS - coord.col.to_zero_based() - 1,
            y: coord.row.to_zero_based(),
        },
    }
}

pub fn to_display_fcoord(p: FCoord, orientation: BoardOrientation) -> DisplayFCoord {
    match orientation {
        BoardOrientation::Normal => DisplayFCoord{ x: p.x, y: (NUM_ROWS as f64) - p.y },
        BoardOrientation::Rotated => DisplayFCoord{ x: (NUM_COLS as f64) - p.x, y: p.y },
    }
}

pub fn from_display_row(y: u8, orientation: BoardOrientation) -> Row {
    match orientation {
        BoardOrientation::Normal => Row::from_zero_based(NUM_ROWS - y - 1),
        BoardOrientation::Rotated => Row::from_zero_based(y),
    }
}

pub fn from_display_col(x: u8, orientation: BoardOrientation) -> Col {
    match orientation {
        BoardOrientation::Normal => Col::from_zero_based(x),
        BoardOrientation::Rotated => Col::from_zero_based(NUM_COLS - x - 1),
    }
}

pub fn from_display_coord(q: DisplayCoord, orientation: BoardOrientation) -> Coord {
    Coord {
        row: from_display_row(q.y, orientation),
        col: from_display_col(q.x, orientation),
    }
}

pub fn display_to_fcoord(q: DisplayFCoord, orientation: BoardOrientation) -> FCoord {
    match orientation {
        BoardOrientation::Normal => FCoord{ x: q.x, y: (NUM_ROWS as f64) - q.y },
        BoardOrientation::Rotated => FCoord{ x: (NUM_COLS as f64) - q.x, y: q.y },
    }
}

impl FCoord {
    // Returns the closes valid board square.
    pub fn to_coord_snapped(&self) -> Coord {
        Coord::new(
            Row::from_zero_based((self.y.clamp(0., (NUM_ROWS - 1) as f64)) as u8),
            Col::from_zero_based((self.x.clamp(0., (NUM_COLS - 1) as f64)) as u8),
        )
    }
}

impl DisplayFCoord {
    // Position of the top-left corner of a square.
    pub fn square_pivot(coord: DisplayCoord) -> Self {
        return DisplayFCoord {
            x: f64::from(coord.x),
            y: f64::from(coord.y),
        }
    }

    pub fn square_center(coord: DisplayCoord) -> Self {
        return DisplayFCoord {
            x: f64::from(coord.x) + 0.5,
            y: f64::from(coord.y) + 0.5,
        }
    }

    pub fn to_square(&self) -> Option<DisplayCoord> {
        let x = self.x as i32;
        let y = self.y as i32;
        if 0 <= x && x < NUM_COLS as i32 && 0 <= y && y < NUM_ROWS as i32 {
            // Improvement potential: clamp instead of asserting the values are in range.
            // Who knows if all browsers guarantee click coords cannot be 0.00001px away?
            Some(DisplayCoord{ x: x.try_into().unwrap(), y: y.try_into().unwrap() })
        } else {
            None
        }
    }
}

// Poor man's 2D geometry. Four vector operation should be enough for everybody.

impl ops::Add<(f64, f64)> for DisplayFCoord {
    type Output = Self;
    fn add(self, (x, y): (f64, f64)) -> Self::Output {
        DisplayFCoord{ x: self.x + x, y: self.y + y }
    }
}

impl ops::Sub for DisplayFCoord {
    type Output = (f64, f64);
    fn sub(self, rhs: DisplayFCoord) -> Self::Output {
        (self.x - rhs.x, self.y - rhs.y)
    }
}

pub fn mult_vec((x, y): (f64, f64), s: f64) -> (f64, f64) {
    (x * s, y * s)
}

pub fn normalize_vec((x, y): (f64, f64)) -> (f64, f64) {
    mult_vec((x, y), 1. / x.hypot(y))
}
