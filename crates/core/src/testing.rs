//! Synthetic 3-map catalogue for tests (no bundled data): A and B overlap, C is disjoint.

use euclid::{Box2D, Transform2D, point2, size2};

use crate::domain::{Map, MapCatalog, MapId, MapImageKey};

fn map(id: &str, bounds: Box2D<f64, crate::domain::Game>) -> Map {
    Map {
        id: MapId::new(id),
        name: id.to_uppercase(),
        image: MapImageKey(format!("maps/{id}.bc7z")),
        image_size: size2(1000.0, 1000.0),
        // 1 px per metre, origin at the bounds' min corner
        game_to_image: Transform2D::translation(-bounds.min.x, -bounds.min.y),
        bounds,
        attribution: None,
        labels: vec![],
        spawns: vec![],
        extracts: vec![],
    }
}

pub fn catalog() -> MapCatalog {
    MapCatalog::try_new(vec![
        map("a", Box2D::new(point2(0.0, 0.0), point2(1000.0, 1000.0))),
        map(
            "b",
            Box2D::new(point2(500.0, 500.0), point2(1500.0, 1500.0)),
        ),
        map(
            "c",
            Box2D::new(point2(5000.0, 5000.0), point2(6000.0, 6000.0)),
        ),
    ])
    .expect("synthetic catalogue is valid")
}

pub fn id(s: &str) -> MapId {
    MapId::new(s)
}
