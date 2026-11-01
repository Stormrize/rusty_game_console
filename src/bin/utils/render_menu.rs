use embedded_graphics::primitives::PrimitiveStyleBuilder;
use embedded_graphics::{
    geometry::Point,
    mono_font::{MonoTextStyle, ascii::FONT_6X10},
    pixelcolor::Rgb565,
    prelude::*,
    primitives::Rectangle,
    text::Text,
};

const MENU_ENTRIES: [&str; 2] = ["Online Chess", "Snake"];

pub fn init_menu<D>(display: &mut D, network_status: &str)
where
    D: DrawTarget<Color = Rgb565>,
{
    let _ = display.clear(Rgb565::new(2, 4, 8));
    let title_style = MonoTextStyle::new(&FONT_6X10, Rgb565::CYAN);
    let normal_style = MonoTextStyle::new(&FONT_6X10, Rgb565::WHITE);
    let hint_style = MonoTextStyle::new(&FONT_6X10, Rgb565::new(18, 42, 24));

    let _ = Text::new("RUST CONSOLE", Point::new(110, 30), title_style).draw(display);
    let _ = Text::new("GAMES", Point::new(146, 48), normal_style).draw(display);

    for (index, entry) in MENU_ENTRIES.into_iter().enumerate() {
        let y = 78 + index as i32 * 42;
        let selected_row = index == 0;

        // rectangle style
        let fill = if selected_row {
            Rgb565::new(5, 35, 24)
        } else {
            Rgb565::new(5, 12, 18)
        };
        let stroke = if selected_row {
            Rgb565::CYAN
        } else {
            Rgb565::new(12, 26, 30)
        };

        // rectangle
        let primitive_style = PrimitiveStyleBuilder::new()
            .fill_color(fill)
            .stroke_color(stroke)
            .stroke_width(1)
            .build();
        let _ = Rectangle::new(Point::new(62, y - 18), Size::new(196, 30))
            .into_styled(primitive_style)
            .draw(display);

        // text
        let text_style = if selected_row {
            title_style
        } else {
            normal_style
        };
        if selected_row {
            let _ = Text::new(">", Point::new(92, y), text_style).draw(display);
        }
        let _ = Text::new(entry, Point::new(104, y), text_style).draw(display);
    }

    let _ = Text::new("UP/DOWN: move", Point::new(12, 200), hint_style).draw(display);
    let _ = Text::new("SELECT: open", Point::new(12, 214), hint_style).draw(display);
    let mut network = heapless::String::<28>::new();
    let _ = core::fmt::Write::write_fmt(&mut network, format_args!("Network: {}", network_status));
    let _ = Text::new(network.as_str(), Point::new(12, 234), normal_style).draw(display);
}

pub fn draw_menu_entry<D>(display: &mut D, selected: usize, network_status: &str)
where
    D: DrawTarget<Color = Rgb565>,
{
    let normal_style = MonoTextStyle::new(&FONT_6X10, Rgb565::WHITE);

    for (index, entry) in MENU_ENTRIES.into_iter().enumerate() {
        let y = 78 + index as i32 * 42;
        let selected_row = index == selected;

        let fill = if selected_row {
            Rgb565::new(5, 35, 24)
        } else {
            Rgb565::new(5, 12, 18)
        };
        let stroke = if selected_row {
            Rgb565::CYAN
        } else {
            Rgb565::new(12, 26, 30)
        };

        let primitive_style = PrimitiveStyleBuilder::new()
            .fill_color(fill)
            .stroke_color(stroke)
            .stroke_width(1)
            .build();
        let _ = Rectangle::new(Point::new(62, y - 18), Size::new(196, 30))
            .into_styled(primitive_style)
            .draw(display);

        let text_style = if selected_row {
            MonoTextStyle::new(&FONT_6X10, Rgb565::CYAN)
        } else {
            MonoTextStyle::new(&FONT_6X10, Rgb565::WHITE)
        };

        if selected_row {
            let _ = Text::new(">", Point::new(92, y), text_style).draw(display);
        }

        let _ = Text::new(entry, Point::new(104, y), text_style).draw(display);
        let mut network = heapless::String::<28>::new();
        let _ =
            core::fmt::Write::write_fmt(&mut network, format_args!("Network: {}", network_status));
        let _ = Text::new(network.as_str(), Point::new(12, 234), normal_style).draw(display);
    }
}
