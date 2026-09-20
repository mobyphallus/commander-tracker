//! Rendering commander art with the per-commander framing (zoom + anchor)
//! the player chose. At zoom 1.0 the art just covers the tile; above that
//! it's blown up and the anchor decides which part stays visible, with the
//! overflow clipped away.

use std::collections::HashMap;

use iced::widget::{container, image, responsive, text};
use iced::{ContentFit, Element, Length};

use crate::model::Commander;

/// Renders `commander`'s art framed to fill whatever space it's given.
/// Falls back to the commander's name while the image is still downloading.
pub fn framed<'a, Msg: 'a>(
    commander: &Commander,
    image_cache: &HashMap<String, image::Handle>,
    placeholder_size: u16,
) -> Element<'a, Msg> {
    let handle = commander
        .portrait_url()
        .and_then(|u| image_cache.get(u))
        .cloned();

    let Some(handle) = handle else {
        let name = commander.name.clone();
        return container(text(name).size(placeholder_size))
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into();
    };

    let zoom = commander.art_zoom.max(1.0);
    let (h_align, v_align) = (
        commander.art_anchor.horizontal(),
        commander.art_anchor.vertical(),
    );

    // `responsive` hands us the tile's real size, which is what lets us
    // scale the image past it and crop back to the tile.
    responsive(move |size| {
        let scaled = image(handle.clone())
            .width(Length::Fixed(size.width * zoom))
            .height(Length::Fixed(size.height * zoom))
            .content_fit(ContentFit::Cover);

        container(
            container(scaled)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(h_align)
                .align_y(v_align),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .clip(true)
        .into()
    })
    .into()
}
