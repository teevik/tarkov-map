//! A Map's fixed mapping from game coordinates to Main Floor image pixels.

use euclid::{Point2D, Size2D, Transform2D, Vector2D};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Game {}
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Image {}

pub type GamePos = Point2D<f64, Game>;
pub type ImagePos = Point2D<f64, Image>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Projection {
    pub game_to_image: Transform2D<f64, Game, Image>,
    pub image_size: Size2D<f64, Image>,
}

impl Projection {
    pub fn project(&self, position: GamePos) -> ImagePos {
        self.game_to_image.transform_point(position)
    }

    /// Yaw is in radians, with zero facing positive game Z.
    pub fn heading(&self, yaw: f64) -> Vector2D<f64, Image> {
        self.game_to_image
            .transform_vector(Vector2D::new(yaw.sin(), yaw.cos()))
            .normalize()
    }

    /// Area-equivalent scale, including Maps with non-uniform scaling.
    pub fn metres_per_pixel(&self) -> f64 {
        1.0 / self.game_to_image.determinant().abs().sqrt()
    }
}
