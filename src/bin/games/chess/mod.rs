pub mod chess_ui;
pub mod network;
pub mod render_chess;

use embedded_graphics::{draw_target::DrawTarget, pixelcolor::Rgb565};

use crate::game::{Control, Game, GameContext};
use crate::games::chess::chess_ui::{is_promotion_move, square_name};
use crate::games::chess::network::{SharedGame, SharedSession};
use crate::games::chess::render_chess::{ChessView, render_chess};
use crate::input::{InputSnapshot, Navigation};

pub struct Chess {
    shared: &'static SharedGame,
    session: &'static SharedSession,
    view: ChessView,
    cursor: (usize, usize),
    selected: Option<(usize, usize)>,
    promotion: usize,
    nav: Navigation,
}

impl Chess {
    pub fn new(shared: &'static SharedGame, session: &'static SharedSession) -> Self {
        Self {
            shared,
            session,
            view: ChessView::new(),
            cursor: (4, 6),
            selected: None,
            promotion: 0,
            nav: Navigation::new(),
        }
    }
}

impl Game for Chess {
    fn init(&mut self) {
        self.view.invalidate();
        self.cursor = (4, 6);
        self.selected = None;
        self.promotion = 0;
    }

    async fn update(&mut self, input: &InputSnapshot, cx: &GameContext) -> Control {
        if self.nav.movement(input) {
            if input.up {
                self.cursor.1 = self.cursor.1.saturating_sub(1);
            }
            if input.down {
                self.cursor.1 = (self.cursor.1 + 1).min(7);
            }
            if input.left {
                self.cursor.0 = self.cursor.0.saturating_sub(1);
            }
            if input.right {
                self.cursor.0 = (self.cursor.0 + 1).min(7);
            }
        }

        if input.press_back {
            if self.selected.is_some() {
                self.selected = None;
                self.shared.lock().await.set_message("Selection cancelled");
            } else {
                return Control::ExitToMenu;
            }
        }

        if input.press_menu {
            self.promotion = (self.promotion + 1) % 4;
            self.shared
                .lock()
                .await
                .set_message("Promotion choice changed");
        }

        if input.press_select {
            if let Some((from_file, from_row)) = self.selected {
                if (from_file, from_row) == self.cursor {
                    self.selected = None;
                } else {
                    let snap = self.shared.lock().await.clone();
                    let from = square_name(from_file, from_row);
                    let to = square_name(self.cursor.0, self.cursor.1);
                    let promotion =
                        if is_promotion_move(snap.fen.as_str(), from_file, from_row, self.cursor.1)
                        {
                            Some(match self.promotion % 4 {
                                0 => 'q',
                                1 => 'r',
                                2 => 'b',
                                _ => 'n',
                            })
                        } else {
                            None
                        };

                    cx.spawner.spawn(
                        network::submit_move_task(
                            *cx.stack,
                            self.shared,
                            self.session,
                            from,
                            to,
                            promotion,
                        )
                        .expect("spawn move task"),
                    );
                    self.selected = None;
                }
            } else {
                self.selected = Some(self.cursor);
            }
        }

        Control::Continue
    }

    async fn render<D>(&mut self, display: &mut D)
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let state = self.shared.lock().await.clone();
        render_chess(
            display,
            &state,
            self.cursor,
            self.selected,
            self.promotion,
            &mut self.view,
        );
    }

    fn spawn_network(&self, cx: &GameContext) {
        network::spawn_network_task(cx.spawner, *cx.stack, cx.chess_shared, self.session);
    }
}
