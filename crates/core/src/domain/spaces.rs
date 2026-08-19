//! Typed coordinate spaces (euclid units). Game = tarkov.dev game metres (x, z);
//! Image = pixels of a Map's Main Floor image; Screen = egui points (app-side only).

use euclid::{Box2D, Point2D, Size2D, Transform2D, Vector2D};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Game;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Image;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Screen;

pub type GamePoint = Point2D<f64, Game>;
pub type GameBox = Box2D<f64, Game>;
pub type ImagePoint = Point2D<f64, Image>;
pub type ImageVector = Vector2D<f64, Image>;
pub type ImageSize = Size2D<f64, Image>;
/// The Projection: one baked affine per Map (ADR-0003).
pub type Projection = Transform2D<f64, Game, Image>;
