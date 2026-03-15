use crate::{
    core::{Layout, Point, Size, layout, mouse, overlay, renderer},
    pane_grid::{self, Catalog},
};

#[derive(Debug, Clone)]
pub struct Indicator {
    pub size: Size,
    pub position: Point,
}

impl<Message, Theme, Renderer> overlay::Overlay<Message, Theme, Renderer> for Indicator
where
    Theme: pane_grid::Catalog,
    Renderer: iced_renderer::core::Renderer,
{
    fn layout(&mut self, _renderer: &Renderer, _bounds: Size) -> layout::Node {
        layout::Node::new(self.size).move_to(self.position)
    }

    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        _inherited_style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
    ) {
        let mut style = pane_grid::Catalog::style(theme, &<Theme as Catalog>::default());
        style.hovered_region.border.radius = 6.0.into();

        renderer.fill_quad(
            renderer::Quad {
                bounds: layout.bounds(),
                border: style.hovered_region.border,
                ..renderer::Quad::default()
            },
            style.hovered_region.background,
        );
    }
}
