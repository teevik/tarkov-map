//! Upstream placement rules stay in the fetcher; the viewer only sees an affine.
use euclid::{Point2D, Size2D, Transform2D};
use tarkov_map::{Game, Image, Projection};

use super::{FetchError, FetchedMap};

#[derive(Clone, Copy, Debug)]
pub enum ImagePlacement {
    Svg { size: [f64; 2], render_scale: f64 },
    Tiles { tile_size: f64 },
}

pub fn bake_projection(
    name: &str,
    map: &FetchedMap,
    placement: ImagePlacement,
    image_size: [f64; 2],
) -> Result<Projection, FetchError> {
    let invalid = |reason: &str| FetchError::InvalidProjection {
        name: name.to_owned(),
        reason: reason.to_owned(),
    };
    let rotation = map
        .coordinate_rotation
        .ok_or_else(|| invalid("missing coordinateRotation"))?;
    if ![0.0, 90.0, 180.0, 270.0].contains(&rotation) {
        return Err(invalid("unsupported coordinateRotation"));
    }
    let (sin, cos) = match rotation as u32 {
        90 => (1.0, 0.0),
        180 => (0.0, -1.0),
        270 => (-1.0, 0.0),
        _ => (0.0, 1.0),
    };
    let [sx, mx, sy, my] = match placement {
        ImagePlacement::Tiles { .. } => map
            .transform
            .ok_or_else(|| invalid("tiles require transform"))?,
        ImagePlacement::Svg { .. } => map.transform.unwrap_or([1.0, 0.0, 1.0, 0.0]),
    };
    if ![sx, mx, sy, my].iter().all(|n| n.is_finite()) || sx <= 0.0 || sy <= 0.0 {
        return Err(invalid(
            "transform must have finite values and positive scales",
        ));
    }
    let crs: Transform2D<f64, Game, Image> =
        Transform2D::new(sx * cos, -sy * sin, -sx * sin, -sy * cos, mx, my);
    let game_to_image = match placement {
        ImagePlacement::Tiles { tile_size } => {
            if !tile_size.is_finite() || tile_size <= 0.0 {
                return Err(invalid("tileSize must be finite and positive"));
            }
            crs.then_scale(image_size[0] / tile_size, image_size[1] / tile_size)
        }
        ImagePlacement::Svg { size, render_scale } => {
            let bounds = map
                .svg_bounds
                .or(map.bounds)
                .ok_or_else(|| invalid("SVG requires bounds"))?;
            if !bounds.iter().flatten().all(|n| n.is_finite())
                || !size.iter().all(|n| n.is_finite() && *n > 0.0)
                || !render_scale.is_finite()
                || render_scale <= 0.0
            {
                return Err(invalid("invalid SVG dimensions or bounds"));
            }
            let a = crs.transform_point(Point2D::new(bounds[0][0], bounds[0][1]));
            let b = crs.transform_point(Point2D::new(bounds[1][0], bounds[1][1]));
            let width = (b.x - a.x).abs();
            let height = (b.y - a.y).abs();
            let scale = (width / size[0]).min(height / size[1]);
            if scale <= 0.0 {
                return Err(invalid("SVG bounds must have non-zero area"));
            }
            // SVGOverlay defaults to xMidYMid meet. Undo its centred padding,
            // then account for the rasterizer's scale (including pixel rounding).
            let left = a.x.min(b.x) + (width - size[0] * scale) / 2.0;
            let top = a.y.min(b.y) + (height - size[1] * scale) / 2.0;
            crs.then(&Transform2D::new(
                render_scale / scale,
                0.0,
                0.0,
                render_scale / scale,
                -left * render_scale / scale,
                -top * render_scale / scale,
            ))
        }
    };
    let projection = Projection {
        game_to_image,
        image_size: Size2D::new(image_size[0], image_size[1]),
    };
    if !image_size.iter().all(|n| n.is_finite() && *n > 0.0)
        || !projection
            .game_to_image
            .to_array()
            .iter()
            .all(|n| n.is_finite())
        || projection.game_to_image.inverse().is_none()
    {
        return Err(invalid(
            "Projection must be finite and invertible with positive image dimensions",
        ));
    }
    Ok(projection)
}

pub fn tile_bounds(projection: &Projection) -> [[f64; 2]; 2] {
    let inverse = projection
        .game_to_image
        .inverse()
        .expect("validated Projection");
    let a = inverse.transform_point(Point2D::new(0.0, 0.0));
    let b = inverse.transform_point(Point2D::new(
        projection.image_size.width,
        projection.image_size.height,
    ));
    super::round_bounds([[a.x.max(b.x), a.y.min(b.y)], [a.x.min(b.x), a.y.max(b.y)]])
}

#[cfg(test)]
#[path = "../../testdata/projection_oracle.rs"]
mod oracle;

#[cfg(test)]
mod tests {
    use super::*;

    fn input(reference: &oracle::OracleMap) -> FetchedMap {
        serde_json::from_value(serde_json::json!({
            "projection": "interactive",
            "coordinateRotation": reference.rotation,
            "transform": reference.transform,
            "bounds": reference.bounds,
            "svgBounds": reference.svg_bounds,
        }))
        .unwrap()
    }

    #[test]
    fn baking_matches_all_38_leaflet_vectors() {
        for reference in oracle::maps() {
            let placement = if reference.tiles {
                ImagePlacement::Tiles {
                    tile_size: reference.source_size[0],
                }
            } else {
                ImagePlacement::Svg {
                    size: reference.source_size,
                    render_scale: 2.0,
                }
            };
            // Include both tile stitching and integer SVG raster dimensions.
            let image_size = reference
                .source_size
                .map(|n| (n * if reference.tiles { 8.0 } else { 2.0 }).floor());
            let projection =
                bake_projection(&reference.name, &input(&reference), placement, image_size)
                    .unwrap();
            oracle::assert_projection(&reference, &projection);
        }
    }

    #[test]
    fn chosen_image_source_controls_projection_even_when_svg_was_available() {
        let reference = oracle::maps()
            .into_iter()
            .find(|m| m.name == "reserve")
            .unwrap();
        let mut map = input(&reference);
        map.svg_path = Some("https://example.test/reserve.svg".to_owned());
        let tiles = bake_projection(
            "reserve",
            &map,
            ImagePlacement::Tiles { tile_size: 256.0 },
            [2048.0, 2048.0],
        )
        .unwrap();
        let origin = tiles.project(tarkov_map::GamePos::new(0.0, 0.0));
        assert_eq!(origin.to_array(), [122.0 * 8.0, 137.65 * 8.0]);
        assert_eq!(tile_bounds(&tiles), [[308.86, -348.48], [-339.24, 299.62]]);
    }

    #[test]
    fn malformed_projection_metadata_fails_instead_of_silently_misplacing_overlays() {
        let reference = oracle::maps().remove(0);
        let mut map = input(&reference);
        let placement = ImagePlacement::Svg {
            size: reference.source_size,
            render_scale: 2.0,
        };
        map.coordinate_rotation = None;
        assert!(bake_projection("customs", &map, placement, [2000.0, 1000.0]).is_err());
        map.coordinate_rotation = Some(45.0);
        assert!(bake_projection("customs", &map, placement, [2000.0, 1000.0]).is_err());
        map.coordinate_rotation = Some(180.0);
        map.bounds = Some([[1.0, 1.0], [1.0, 1.0]]);
        assert!(bake_projection("customs", &map, placement, [2000.0, 1000.0]).is_err());
        map.transform = None;
        assert!(
            bake_projection(
                "customs",
                &map,
                ImagePlacement::Tiles { tile_size: 256.0 },
                [2048.0, 2048.0]
            )
            .is_err()
        );
    }
}
