pub const WIDTH: u8 = 20;
pub const HEIGHT: u8 = 16;
const MAX_LEN: usize = 192;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

pub struct SnakeGame {
    body: [(u8, u8); MAX_LEN],
    len: usize,
    direction: Direction,
    moving: Direction,
    food: (u8, u8),
    over: bool,
    score: u16,
    seed: u32,
}

impl SnakeGame {
    pub fn new() -> Self {
        let mut body = [(0, 0); MAX_LEN];
        body[0] = (10, 8);
        body[1] = (9, 8);
        body[2] = (8, 8);
        Self {
            body,
            len: 3,
            direction: Direction::Right,
            moving: Direction::Right,
            food: (14, 8),
            over: false,
            score: 0,
            seed: 0x51A7_2026,
        }
    }

    pub fn reset(&mut self) {
        let seed = self.seed.wrapping_add(0x9E37_79B9);
        *self = Self::new();
        self.seed = seed;
        self.place_food();
    }

    /// Возвращает true, если направление принято (не разворот и игра не окончена).
    pub fn set_direction(&mut self, direction: Direction) -> bool {
        let is_reverse = matches!(
            (self.moving, direction),
            (Direction::Up, Direction::Down)
                | (Direction::Down, Direction::Up)
                | (Direction::Left, Direction::Right)
                | (Direction::Right, Direction::Left)
        );

        if is_reverse || self.over {
            return false;
        }
        self.direction = direction;
        true
    }

    pub fn tick(&mut self) {
        if self.over {
            return;
        }
        self.moving = self.direction;

        let (x, y) = self.body[0];
        let next = match self.direction {
            Direction::Up if y > 0 => (x, y - 1),
            Direction::Down if y + 1 < HEIGHT => (x, y + 1),
            Direction::Left if x > 0 => (x - 1, y),
            Direction::Right if x + 1 < WIDTH => (x + 1, y),
            _ => {
                self.over = true;
                return;
            }
        };

        let eating = next == self.food;
        // The tail vacates its square on a normal move, but not while growing.
        let collision_len = if eating {
            self.len
        } else {
            self.len.saturating_sub(1)
        };
        if self.body[..collision_len].contains(&next) {
            self.over = true;
            return;
        }

        if eating {
            if self.len >= MAX_LEN {
                self.over = true;
                return;
            }
            self.len += 1;
            self.score = self.score.saturating_add(1);
        }
        for index in (1..self.len).rev() {
            self.body[index] = self.body[index - 1];
        }
        self.body[0] = next;

        if eating {
            self.place_food();
        }
    }

    fn place_food(&mut self) {
        // Deterministic PRNG is enough for this tiny offline game and avoids heap/RNG setup.
        for _ in 0..640 {
            self.seed = self
                .seed
                .wrapping_mul(1_664_525)
                .wrapping_add(1_013_904_223);
            let r = self.seed >> 8;
            let candidate = (
                (r % WIDTH as u32) as u8,
                ((r / WIDTH as u32) % HEIGHT as u32) as u8,
            );
            if !self.body[..self.len].contains(&candidate) {
                self.food = candidate;
                return;
            }
        }
        self.over = true;
    }

    pub fn body(&self) -> &[(u8, u8)] {
        &self.body[..self.len]
    }

    pub fn food(&self) -> (u8, u8) {
        self.food
    }

    pub fn score(&self) -> u16 {
        self.score
    }

    pub fn is_over(&self) -> bool {
        self.over
    }
}
