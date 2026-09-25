//! Rendering commander art framed to a tile.
//!
//! The geometry lives here; the drawing is done by `panned_image`, which
//! explains why it takes the shape it does.

use std::collections::HashMap;

use iced::widget::{column, container, image, responsive, row, Space};
use iced::{Element, Length, Rectangle, Size};

use crate::model::{ArtFraming, Commander};
use crate::panned_image;

/// Where the art actually lands inside `tile`, given its framing.
///
/// The art is first scaled to *cover* the tile, then multiplied by zoom.
/// Pan is a fraction of the maximum travel, so +/-1 is exactly far enough to
/// bring an edge flush and never further - the tile can never show a gap.
pub fn placement(tile: Size, image_size: Size, framing: ArtFraming) -> Rectangle {
    let framing = framing.clamped();
    let cover = (tile.width / image_size.width).max(tile.height / image_size.height);
    let scale = cover * framing.zoom;
    let drawn = Size::new(image_size.width * scale, image_size.height * scale);

    // How far the art can slide before an edge would come into view.
    let slack_x = ((drawn.width - tile.width) / 2.0).max(0.0);
    let slack_y = ((drawn.height - tile.height) / 2.0).max(0.0);

    Rectangle {
        x: (tile.width - drawn.width) / 2.0 + framing.pan_x * slack_x,
        y: (tile.height - drawn.height) / 2.0 + framing.pan_y * slack_y,
        width: drawn.width,
        height: drawn.height,
    }
}

/// The pixel dimensions of a handle, needed to work out how the art covers
/// a tile. Art is decoded when it's cached precisely so this is exact; the
/// fallback is Scryfall's art-crop shape, for a handle that somehow isn't.
pub fn image_dimensions(handle: &image::Handle) -> Size {
    match handle {
        image::Handle::Rgba { width, height, .. } => Size::new(*width as f32, *height as f32),
        _ => Size::new(626.0, 457.0),
    }
}

/// Renders `commander`'s art framed to fill whatever space it's given.
/// Falls back to the commander's name while the image is still downloading.
pub fn framed<'a, Msg: 'a>(
    commander: &Commander,
    image_cache: &HashMap<String, image::Handle>,
    _placeholder_size: u16,
    rotation: f32,
) -> Element<'a, Msg> {
    let handle = commander
        .portrait_url()
        .and_then(|u| image_cache.get(u))
        .cloned();

    let Some(handle) = handle else {
        // The seat already carries its name in a rotated caption. A second
        // unrotated name here would show through the life counter's scrim.
        return container(Space::new(Length::Fill, Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into();
    };

    panned_image::display(handle, commander.framing, rotation)
}

/// Split along the player's longer dimension, keeping both portraits useful
/// even in tall seats. The primary stays first from that player's viewpoint.
fn pair_layout(size: Size, rotation: f32) -> (bool, bool) {
    let (sin, cos) = rotation.sin_cos();
    let sideways = sin.abs() > 0.5;
    let facing = if sideways {
        Size::new(size.height, size.width)
    } else {
        size
    };
    let across = facing.width > facing.height * 1.35;
    let horizontal = across != sideways;
    let reverse = if across {
        if sideways {
            sin < 0.0
        } else {
            cos < 0.0
        }
    } else if sideways {
        sin > 0.0
    } else {
        cos < 0.0
    };
    (horizontal, reverse)
}

/// Partner commanders share equal, edge-to-edge panels with a quiet divider.
/// Each portrait retains its own framing and faces the seat's player.
pub fn framed_pair<'a, Msg: 'a>(
    commander: &Commander,
    partner: Option<&Commander>,
    image_cache: &HashMap<String, image::Handle>,
    placeholder_size: u16,
    rotation: f32,
) -> Element<'a, Msg> {
    let Some(partner) = partner else {
        return framed(commander, image_cache, placeholder_size, rotation);
    };
    let portraits = [commander, partner].map(|c| {
        (
            c.portrait_url()
                .and_then(|url| image_cache.get(url))
                .cloned(),
            c.framing,
        )
    });
    responsive(move |size| {
        let (horizontal, reverse) = pair_layout(size, rotation);
        let panel = |index: usize| -> Element<'a, Msg> {
            let (handle, framing) = &portraits[index];
            let art = match handle {
                Some(handle) => panned_image::display(handle.clone(), *framing, rotation),
                None => Space::new(Length::Fill, Length::Fill).into(),
            };
            container(art)
                .width(Length::Fill)
                .height(Length::Fill)
                .clip(true)
                .into()
        };
        let (first, second) = if reverse { (1, 0) } else { (0, 1) };
        if horizontal {
            row![panel(first), panel(second)]
                .spacing(3)
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else {
            column![panel(first), panel(second)]
                .spacing(3)
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        }
    })
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TILE: Size = Size {
        width: 400.0,
        height: 300.0,
    };
    const ART: Size = Size {
        width: 626.0,
        height: 457.0,
    };

    #[test]
    fn partner_panels_follow_the_players_orientation() {
        use std::f32::consts::{FRAC_PI_2, PI};
        let tall = Size::new(400.0, 600.0);
        let wide = Size::new(600.0, 400.0);
        assert_eq!(pair_layout(tall, 0.0), (false, false));
        assert_eq!(pair_layout(tall, PI), (false, true));
        assert_eq!(pair_layout(wide, FRAC_PI_2), (true, true));
        assert_eq!(pair_layout(wide, -FRAC_PI_2), (true, false));
        assert_eq!(pair_layout(wide, 0.0), (true, false));
        assert_eq!(pair_layout(tall, FRAC_PI_2), (false, false));
    }

    /// At rest the art must cover the tile exactly, with no gap on any edge.
    #[test]
    fn default_framing_covers_the_tile() {
        let r = placement(TILE, ART, ArtFraming::default());
        assert!(
            r.x <= 0.01 && r.y <= 0.01,
            "art starts inside the tile: {r:?}"
        );
        assert!(r.x + r.width >= TILE.width - 0.01);
        assert!(r.y + r.height >= TILE.height - 0.01);
    }

    /// Panning to either extreme brings an edge flush and never past it,
    /// so a tile can never show background through the art.
    #[test]
    fn pan_never_uncovers_an_edge() {
        for (px, py) in [(-1.0, -1.0), (1.0, 1.0), (-1.0, 1.0), (1.0, -1.0)] {
            for zoom in [1.0, 1.5, 3.0] {
                let r = placement(
                    TILE,
                    ART,
                    ArtFraming {
                        zoom,
                        pan_x: px,
                        pan_y: py,
                    },
                );
                assert!(r.x <= 0.01, "left gap at {px},{py} zoom {zoom}: {r:?}");
                assert!(r.y <= 0.01, "top gap at {px},{py} zoom {zoom}: {r:?}");
                assert!(r.x + r.width >= TILE.width - 0.01, "right gap: {r:?}");
                assert!(r.y + r.height >= TILE.height - 0.01, "bottom gap: {r:?}");
            }
        }
    }

    /// Out-of-range values are clamped rather than trusted.
    #[test]
    fn framing_is_clamped() {
        let r = placement(
            TILE,
            ART,
            ArtFraming {
                zoom: 99.0,
                pan_x: 50.0,
                pan_y: -50.0,
            },
        );
        assert!(r.x <= 0.01 && r.x + r.width >= TILE.width - 0.01);
        assert!(r.y <= 0.01 && r.y + r.height >= TILE.height - 0.01);
    }

    /// A tall tile and a wide tile keep very different amounts of the same
    /// art on screen - which is exactly why framing is stored per layout and
    /// seat rather than once per commander.
    #[test]
    fn tile_shape_changes_how_much_art_is_visible() {
        let wide_tile = Size::new(600.0, 200.0);
        let tall_tile = Size::new(200.0, 600.0);
        let wide = placement(wide_tile, ART, ArtFraming::default());
        let tall = placement(tall_tile, ART, ArtFraming::default());

        // Fraction of the art's own width left visible in each tile.
        let wide_visible = wide_tile.width / wide.width;
        let tall_visible = tall_tile.width / tall.width;
        assert!(
            wide_visible > tall_visible + 0.3,
            "wide tile should show much more of the art's width \
             (wide {wide_visible}, tall {tall_visible})"
        );
    }
}
