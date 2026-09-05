//! Placement in the Viewport uses the Map's precomputed Projection.
use eframe::egui;
use tarkov_map::{GamePos, Map};

pub fn game_to_display(map: &Map, map_rect: egui::Rect, game_pos: [f64; 2]) -> Option<egui::Pos2> {
    let point = map
        .projection
        .project(GamePos::new(game_pos[0], game_pos[1]));
    let size = map.projection.image_size;
    Some(egui::pos2(
        map_rect.min.x + (point.x / size.width) as f32 * map_rect.width(),
        map_rect.min.y + (point.y / size.height) as f32 * map_rect.height(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserve_d2_stays_aligned_when_the_viewport_is_panned_and_zoomed() {
        let maps: Vec<Map> = ron::from_str(include_str!("../../../assets/maps.ron")).unwrap();
        let reserve = maps
            .iter()
            .find(|m| m.normalized_name == "reserve")
            .unwrap();
        for (origin, zoom) in [
            (egui::pos2(0.0, 0.0), 1.0),
            (egui::pos2(-300.0, 170.0), 3.0),
        ] {
            let rect = egui::Rect::from_min_size(origin, egui::vec2(1654.0, 1522.0) * zoom);
            let d2 = game_to_display(reserve, rect, [-121.479065, 172.24913]).unwrap();
            // Independent Leaflet oracle in SVG units, times the 2x render scale.
            let expected = origin + egui::vec2(573.6219, 622.6883) * 2.0 * zoom;
            assert!(d2.distance(expected) < 0.01, "{d2:?} vs {expected:?}");
        }
    }
}
