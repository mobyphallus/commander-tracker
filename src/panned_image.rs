//! An image that fills its box, cropped, and can be dragged around inside it.
//!
//! This exists because neither of the obvious approaches works in iced 0.13:
//!
//! - An oversized `image` inside an aligned container cannot pan. A container
//!   aligns a child within its *free* space, and a child larger than the
//!   container has none, so the art pins to the top-left however it's aligned.
//! - Drawing through a `Canvas` can pan, but cannot be clipped. The wgpu
//!   backend's `paste` copies geometry images out with `images.extend(..)`
//!   and only applies clip bounds to meshes, so canvas art spills over
//!   whatever is next to it - neighbouring seats, in our case.
//!
//! So this draws the image at an explicit rectangle inside `with_layer`,
//! which is the same call the stock image widget uses to clip an oversized
//! `ContentFit::Cover`, and is properly bounded.

use iced::advanced::widget::{tree, Tree, Widget};
use iced::advanced::{image, layout, mouse, renderer, Clipboard, Layout, Shell};
use iced::{event, touch, Element, Event, Length, Point, Rectangle, Size};

use crate::art;
use crate::model::{ArtFraming, MAX_ART_ZOOM, MIN_ART_ZOOM};

/// How much one notch of scroll wheel changes the zoom.
const WHEEL_STEP: f32 = 0.12;

#[derive(Default)]
struct Gesture {
    /// Where a one-finger / mouse drag was last seen.
    drag_from: Option<Point>,
    /// Live touches, so two fingers can be told from one.
    touches: Vec<(touch::Finger, Point)>,
    /// Finger separation and zoom at the moment a pinch began.
    pinch: Option<(f32, f32)>,
}

fn distance(a: Point, b: Point) -> f32 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
}

pub struct PannedImage<'a, Message> {
    handle: image::Handle,
    framing: ArtFraming,
    /// Clockwise rotation in radians, so a seat's art faces the player
    /// sitting at that edge of the table.
    rotation: f32,
    /// Set only for the framing editor; None makes this a plain display.
    on_change: Option<Box<dyn Fn(ArtFraming) -> Message + 'a>>,
}

/// The box the art has to cover, from the point of view of the player
/// looking at it. A seat turned on its side sees the tile's width and
/// height swapped, and the art has to cover *that* to leave no gap.
fn facing_size(bounds: Size, rotation: f32) -> Size {
    let quarter_turns = (rotation / std::f32::consts::FRAC_PI_2).round() as i32;
    if quarter_turns.rem_euclid(2) == 1 {
        Size::new(bounds.height, bounds.width)
    } else {
        bounds
    }
}

impl<Message> PannedImage<'_, Message> {
    /// Moves the art by a pixel delta, converted into pan fractions using
    /// the travel actually available at this zoom and box shape. With no
    /// travel on an axis (the art exactly fits it) that axis can't move.
    fn panned(&self, bounds: Size, dx: f32, dy: f32) -> ArtFraming {
        let facing = facing_size(bounds, self.rotation);
        let rect = art::placement(facing, art::image_dimensions(&self.handle), self.framing);
        let slack_x = ((rect.width - facing.width) / 2.0).max(0.0);
        let slack_y = ((rect.height - facing.height) / 2.0).max(0.0);

        let mut next = self.framing;
        if slack_x > 0.5 {
            next.pan_x += dx / slack_x;
        }
        if slack_y > 0.5 {
            next.pan_y += dy / slack_y;
        }
        next.clamped()
    }

    fn zoomed(&self, factor: f32) -> ArtFraming {
        let mut next = self.framing;
        next.zoom = (next.zoom * factor).clamp(MIN_ART_ZOOM, MAX_ART_ZOOM);
        next.clamped()
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for PannedImage<'_, Message>
where
    Renderer: image::Renderer<Handle = image::Handle>,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Gesture>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Gesture::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(
        &self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::Node::new(limits.max())
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let measured = renderer.measure_image(&self.handle);
        let image_size = Size::new(measured.width as f32, measured.height as f32);

        let facing = facing_size(bounds.size(), self.rotation);
        let local = art::placement(facing, image_size, self.framing);

        // How far the art sits from the middle, in the player's own frame.
        // The renderer spins the quad about its own centre, so this offset
        // has to be spun with it or the pan would slide the wrong way.
        let offset_x = local.x + local.width / 2.0 - facing.width / 2.0;
        let offset_y = local.y + local.height / 2.0 - facing.height / 2.0;
        let (sin, cos) = self.rotation.sin_cos();
        let spun_x = offset_x * cos - offset_y * sin;
        let spun_y = offset_x * sin + offset_y * cos;

        let center_x = bounds.x + bounds.width / 2.0 + spun_x;
        let center_y = bounds.y + bounds.height / 2.0 + spun_y;
        let drawing_bounds = Rectangle {
            x: center_x - local.width / 2.0,
            y: center_y - local.height / 2.0,
            width: local.width,
            height: local.height,
        };

        // Always layered: the art is deliberately bigger than the box, and
        // this is what keeps it from drawing over the neighbouring seats.
        renderer.with_layer(bounds, |renderer| {
            renderer.draw_image(
                image::Image {
                    handle: self.handle.clone(),
                    filter_method: image::FilterMethod::Linear,
                    rotation: iced::Radians(self.rotation),
                    opacity: 1.0,
                    snap: true,
                },
                drawing_bounds,
            );
        });
    }

    fn on_event(
        &mut self,
        tree: &mut Tree,
        event: Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) -> event::Status {
        let Some(on_change) = &self.on_change else {
            return event::Status::Ignored;
        };
        let bounds = layout.bounds();
        let state = tree.state.downcast_mut::<Gesture>();

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let Some(position) = cursor.position_in(bounds) else {
                    return event::Status::Ignored;
                };
                state.drag_from = Some(position);
                event::Status::Captured
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                state.drag_from = None;
                event::Status::Captured
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let (Some(from), Some(now)) = (state.drag_from, cursor.position_in(bounds)) else {
                    return event::Status::Ignored;
                };
                state.drag_from = Some(now);
                shell.publish(on_change(self.panned(
                    bounds.size(),
                    now.x - from.x,
                    now.y - from.y,
                )));
                event::Status::Captured
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                if cursor.position_in(bounds).is_none() {
                    return event::Status::Ignored;
                }
                let lines = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => y,
                    mouse::ScrollDelta::Pixels { y, .. } => y / 40.0,
                };
                if lines == 0.0 {
                    return event::Status::Ignored;
                }
                shell.publish(on_change(self.zoomed(1.0 + lines * WHEEL_STEP)));
                event::Status::Captured
            }
            Event::Touch(touch_event) => {
                match touch_event {
                    touch::Event::FingerPressed { id, position } => {
                        if !bounds.contains(position) {
                            return event::Status::Ignored;
                        }
                        state.touches.push((id, position));
                        state.pinch = None;
                        state.drag_from = (state.touches.len() == 1).then_some(position);
                    }
                    touch::Event::FingerLifted { id, .. } | touch::Event::FingerLost { id, .. } => {
                        if !state.touches.iter().any(|(f, _)| *f == id) {
                            return event::Status::Ignored;
                        }
                        state.touches.retain(|(f, _)| *f != id);
                        state.pinch = None;
                        state.drag_from = state.touches.first().map(|(_, p)| *p);
                    }
                    touch::Event::FingerMoved { id, position } => {
                        let Some(slot) = state.touches.iter_mut().find(|(f, _)| *f == id) else {
                            return event::Status::Ignored;
                        };
                        slot.1 = position;

                        // Two fingers pinch to zoom, measured against the
                        // separation when the second finger landed, so the
                        // zoom tracks the gesture instead of drifting.
                        if let [(_, a), (_, b)] = state.touches.as_slice() {
                            let spread = distance(*a, *b);
                            let (start_spread, start_zoom) = *state
                                .pinch
                                .get_or_insert((spread.max(1.0), self.framing.zoom));
                            let mut next = self.framing;
                            next.zoom = (start_zoom * (spread / start_spread))
                                .clamp(MIN_ART_ZOOM, MAX_ART_ZOOM);
                            shell.publish(on_change(next.clamped()));
                            return event::Status::Captured;
                        }

                        // One finger drags.
                        if let Some(from) = state.drag_from {
                            state.drag_from = Some(position);
                            shell.publish(on_change(self.panned(
                                bounds.size(),
                                position.x - from.x,
                                position.y - from.y,
                            )));
                        }
                    }
                }
                event::Status::Captured
            }
            _ => event::Status::Ignored,
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        if self.on_change.is_none() {
            return mouse::Interaction::default();
        }
        let state = tree.state.downcast_ref::<Gesture>();
        if state.drag_from.is_some() {
            mouse::Interaction::Grabbing
        } else if cursor.is_over(layout.bounds()) {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::default()
        }
    }
}

impl<'a, Message, Theme, Renderer> From<PannedImage<'a, Message>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: image::Renderer<Handle = image::Handle> + 'a,
{
    fn from(widget: PannedImage<'a, Message>) -> Self {
        Element::new(widget)
    }
}

/// A cropped, non-interactive view of `handle` framed by `framing`, turned
/// `rotation` radians clockwise to face its player.
pub fn display<'a, Message: 'a>(
    handle: image::Handle,
    framing: ArtFraming,
    rotation: f32,
) -> Element<'a, Message> {
    PannedImage {
        handle,
        framing,
        rotation,
        on_change: None,
    }
    .into()
}

/// The same, but draggable: `on_change` fires continuously while the art is
/// being dragged, pinched or scrolled.
pub fn editable<'a, Message: 'a>(
    handle: image::Handle,
    framing: ArtFraming,
    on_change: impl Fn(ArtFraming) -> Message + 'a,
) -> Element<'a, Message> {
    PannedImage {
        handle,
        framing,
        // The editor always works the right way up: you frame what the
        // player will see, and the seat's own rotation is applied on top.
        rotation: 0.0,
        on_change: Some(Box::new(on_change)),
    }
    .into()
}
