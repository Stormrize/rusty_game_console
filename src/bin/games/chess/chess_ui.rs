pub fn fen_to_grid(fen: &str) -> [[u8; 8]; 8] {
    let mut grid = [[b' '; 8]; 8];
    let Some(placement) = fen.split_whitespace().next() else {
        return grid;
    };
    let mut row = 0usize;
    let mut col = 0usize;
    for b in placement.bytes() {
        match b {
            b'/' => {
                row = row.saturating_add(1);
                col = 0;
            }
            b'1'..=b'8' => {
                col = col.saturating_add((b - b'0') as usize);
            }
            b'A'..=b'Z' | b'a'..=b'z' => {
                if row < 8 && col < 8 {
                    grid[row][col] = b;
                }
                col = col.saturating_add(1);
            }
            _ => {}
        }
    }
    grid
}

pub fn square_name(file: usize, row: usize) -> [u8; 2] {
    [b'a' + file.min(7) as u8, b'8' - row.min(7) as u8]
}

pub fn is_promotion_move(fen: &str, from_file: usize, from_row: usize, to_row: usize) -> bool {
    let grid = fen_to_grid(fen);
    if from_file >= 8 || from_row >= 8 || to_row >= 8 {
        return false;
    }
    let piece = grid[from_row][from_file];
    (piece == b'P' && to_row == 0) || (piece == b'p' && to_row == 7)
}
