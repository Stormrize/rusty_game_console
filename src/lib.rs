#![no_std]

use embedded_graphics::{
    Pixel, draw_target::DrawTarget, geometry::Dimensions, pixelcolor::PixelColor,
};

// transparet filter, it works, but computing overhead is huge
pub struct TransparentFilter<'a, D, C> {
    parent: &'a mut D,
    transparent_color: C,
}

impl<'a, D, C> TransparentFilter<'a, D, C>
where
    D: DrawTarget<Color = C>,
    C: PixelColor + PartialEq,
{
    pub fn new(display: &'a mut D, transparent_color: C) -> Self {
        Self {
            parent: display,
            transparent_color,
        }
    }
}

impl<'a, D, C> DrawTarget for TransparentFilter<'a, D, C>
where
    D: DrawTarget<Color = C>,
    C: PixelColor + PartialEq,
{
    type Color = C;
    type Error = D::Error;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        // Главная магия: фильтруем пиксели, пропуская только те,
        // цвет которых НЕ равен прозрачному.
        let filtered_pixels = pixels
            .into_iter()
            .filter(|Pixel(_, color)| *color != self.transparent_color);

        self.parent.draw_iter(filtered_pixels)
    }
}

// Реализуем Dimensions, чтобы адаптер знал размеры оригинального экрана
impl<'a, D, C> Dimensions for TransparentFilter<'a, D, C>
where
    D: DrawTarget<Color = C>,
{
    fn bounding_box(&self) -> embedded_graphics::primitives::Rectangle {
        self.parent.bounding_box()
    }
}
