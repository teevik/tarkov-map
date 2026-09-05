#[path = "../src/testdata/projection_oracle.rs"]
mod oracle;

use tarkov_map::{GamePos, MapCatalog, TarkovMaps};

fn maps() -> TarkovMaps {
    let maps = ron::from_str(include_str!("../assets/maps.ron")).unwrap();
    MapCatalog::try_new(maps).unwrap().into_maps()
}

#[test]
fn bundled_projections_match_leaflet_oracle() {
    let maps = maps();
    for reference in oracle::maps() {
        let map = maps
            .iter()
            .find(|m| m.normalized_name == reference.name)
            .unwrap();
        oracle::assert_projection(&reference, &map.projection);
    }
}

#[test]
fn projection_dimensions_match_bundled_images() {
    for map in maps() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join(map.image_path);
        let bytes = std::fs::read(path).unwrap();
        let image = tarkov_map::bc7z::unpack(&bytes).unwrap();
        assert_eq!(
            map.projection.image_size.to_array(),
            image.pixel_size.map(f64::from)
        );
    }
}

#[test]
fn headings_follow_the_same_projection_as_positions() {
    for map in maps() {
        for yaw in [0.0_f64, std::f64::consts::FRAC_PI_2, 0.7] {
            let origin = map.projection.project(GamePos::new(0.0, 0.0));
            let ahead = map.projection.project(GamePos::new(yaw.sin(), yaw.cos()));
            let expected = (ahead - origin).normalize();
            let heading = map.projection.heading(yaw);
            assert!((heading - expected).length() < 1e-10, "{}", map.name);
        }
        let zero = map.projection.heading(0.0);
        match map.normalized_name.as_str() {
            "factory" => assert!(zero.x < -0.999 && zero.y.abs() < 1e-10),
            "reserve" | "icebreaker" => assert!(zero.y > 0.999 && zero.x.abs() < 1e-10),
            _ => {}
        }
    }
}

#[test]
fn icebreaker_bounds_come_from_its_image_and_reserve_keeps_playable_bounds() {
    let maps = maps();
    let icebreaker = maps
        .iter()
        .find(|m| m.normalized_name == "icebreaker")
        .unwrap();
    assert_eq!(icebreaker.bounds, Some([[62.5, -26.0], [-65.5, 47.14]]));
    let reserve = maps
        .iter()
        .find(|m| m.normalized_name == "reserve")
        .unwrap();
    assert_eq!(reserve.bounds, Some([[289.0, -293.0], [-303.0, 244.0]]));
}
