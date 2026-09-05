//! Shared inputs and independently recorded Leaflet results (four decimal places).
use serde::Deserialize;
use tarkov_map::{GamePos, Projection};

#[derive(Deserialize)]
#[allow(dead_code)] // Source fields are used by fetch tests; the bundle test reads the vectors.
pub struct OracleMap {
    pub name: String,
    pub source_size: [f64; 2],
    pub tiles: bool,
    pub rotation: f64,
    pub transform: [f64; 4],
    pub bounds: [[f64; 2]; 2],
    pub svg_bounds: Option<[[f64; 2]; 2]>,
    pub vectors: Vec<OracleVector>,
}

#[derive(Deserialize)]
pub struct OracleVector {
    pub game: [f64; 2],
    pub fraction: [f64; 2],
}

pub fn maps() -> Vec<OracleMap> {
    let maps: Vec<OracleMap> = ron::from_str(include_str!("projection-oracle.ron")).unwrap();
    assert_eq!(maps.iter().map(|m| m.vectors.len()).sum::<usize>(), 38);
    maps
}

pub fn assert_projection(oracle: &OracleMap, projection: &Projection) {
    let size = if oracle.tiles {
        projection.image_size.to_array()
    } else {
        // SVG PNGs are rendered at 2x then truncated to integer pixels. The
        // oracle fractions refer to the complete SVG before that subpixel crop.
        oracle.source_size.map(|n| n * 2.0)
    };
    for vector in &oracle.vectors {
        let point = projection.project(GamePos::new(vector.game[0], vector.game[1]));
        let actual = [point.x / size[0], point.y / size[1]];
        for (axis, value) in actual.iter().enumerate() {
            assert!(
                (value - vector.fraction[axis]).abs() < 0.00006,
                "{} {:?} axis {axis}: expected {}, got {}",
                oracle.name,
                vector.game,
                vector.fraction[axis],
                actual[axis],
            );
        }
    }
}
