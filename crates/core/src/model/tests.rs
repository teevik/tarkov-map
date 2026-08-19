use std::time::{Duration, SystemTime};

use euclid::{Angle, point2};

use super::*;
use crate::ports::{PositionEvent, SourceKind};
use crate::testing::{catalog, id};

const T0: SystemTime = SystemTime::UNIX_EPOCH;

fn at(secs: u64) -> SystemTime {
    T0 + Duration::from_secs(secs)
}

fn pos(x: f64, z: f64, secs: u64) -> PlayerPosition {
    PlayerPosition {
        ground: point2(x, z),
        height: 0.0,
        heading: Angle::zero(),
        taken_at: at(secs),
    }
}

fn fresh() -> Model {
    let (m, effects) = Model::new(catalog(), Settings::default(), Some("/shots".into()), T0);
    assert_eq!(
        effects,
        vec![
            Effect::DecodeImage(id("a")),
            Effect::WatchScreenshots("/shots".into())
        ]
    );
    m
}

fn dump(m: &Model) -> String {
    format!(
        "selected={} outgoing={:?} image={:?} zoom={} positions={} trail={} tracking={:?} suggestion={:?} muted={} notifications={}",
        m.selected(),
        m.outgoing(),
        m.image(),
        m.viewport().zoom,
        m.positions.len(),
        m.trail().count(),
        m.tracking(),
        m.suggestion().map(|s| &s.id),
        m.suggestion_muted,
        m.notifications().len(),
    )
}

#[test]
fn unknown_settings_map_falls_back_to_first() {
    let settings = Settings {
        selected_map: Some(id("nope")),
        ..Default::default()
    };
    let (m, _) = Model::new(catalog(), settings, None, T0);
    assert_eq!(m.selected(), &id("a"));
    assert!(matches!(
        m.tracking(),
        Tracking::Off(WatchError::NoDocumentsFolder)
    ));
}

#[test]
fn reselect_is_a_no_op_and_switch_resets_and_persists() {
    let mut m = fresh();
    assert!(m.handle(Msg::SelectMap(id("a")), at(1)).is_empty());
    let effects = m.handle(Msg::SelectMapByIndex(1), at(1));
    assert_eq!(effects[0], Effect::DecodeImage(id("b")));
    assert!(
        matches!(effects[1], Effect::PersistSettings(ref s) if s.selected_map == Some(id("b")))
    );
    assert_eq!(m.outgoing(), Some(&id("a")));
    assert!(m.handle(Msg::SelectMapByIndex(99), at(1)).is_empty());
    println!("{}", dump(&m));
}

#[test]
fn stale_decode_for_unselected_map_is_dropped() {
    let mut m = fresh();
    m.handle(Msg::SelectMap(id("b")), at(1));
    let img = crate::ports::fakes::FakeDecoder::instant().image;
    assert!(
        m.handle(
            Msg::ImageDecoded {
                map: id("a"),
                image: img.clone()
            },
            at(2)
        )
        .is_empty()
    );
    let effects = m.handle(
        Msg::ImageDecoded {
            map: id("b"),
            image: img.clone(),
        },
        at(2),
    );
    assert_eq!(
        effects,
        vec![Effect::PresentImage {
            map: id("b"),
            image: img
        }]
    );
    assert_eq!(m.image(), &MapImage::Ready { since: at(2) });
}

#[test]
fn positions_dedupe_order_cap_and_clear_on_started() {
    let mut m = fresh();
    m.handle(
        Msg::PositionEvent(PositionEvent::Started(SourceKind::Screenshots)),
        at(0),
    );
    assert!(matches!(
        m.tracking(),
        Tracking::Waiting(SourceKind::Screenshots)
    ));
    for s in 1..=25 {
        m.handle(
            Msg::PositionEvent(PositionEvent::Position(pos(10.0, 10.0 + s as f64, s))),
            at(s),
        );
    }
    assert_eq!(m.positions.len(), TRAIL_CAPACITY);
    assert_eq!(m.trail().count(), TRAIL_CAPACITY - 1);
    // identical and older are ignored
    m.handle(
        Msg::PositionEvent(PositionEvent::Position(pos(10.0, 35.0, 25))),
        at(26),
    );
    m.handle(
        Msg::PositionEvent(PositionEvent::Position(pos(1.0, 1.0, 3))),
        at(26),
    );
    assert_eq!(m.current_position().unwrap().taken_at, at(25));
    assert!(matches!(m.tracking(), Tracking::Positioned(_)));
    println!("{}", dump(&m));
    // restart clears everything, so an older-mtime screenshot is accepted again
    m.handle(
        Msg::PositionEvent(PositionEvent::Started(SourceKind::Demo)),
        at(30),
    );
    assert!(m.current_position().is_none());
    m.handle(
        Msg::PositionEvent(PositionEvent::Position(pos(1.0, 1.0, 3))),
        at(30),
    );
    assert_eq!(m.current_position().unwrap().taken_at, at(3));
}

#[test]
fn switch_keeps_only_newest_position() {
    let mut m = fresh();
    for s in 1..=5 {
        m.handle(
            Msg::PositionEvent(PositionEvent::Position(pos(1.0, 1.0, s))),
            at(s),
        );
    }
    m.handle(Msg::SelectMap(id("c")), at(6));
    assert_eq!(m.positions.len(), 1);
    assert_eq!(m.trail().count(), 0);
}

#[test]
fn suggestion_only_for_exactly_one_candidate_and_mutes_until_back_inside() {
    let mut m = fresh(); // on "a"
    // inside a and b → no suggestion (inside selected)
    m.handle(
        Msg::PositionEvent(PositionEvent::Position(pos(700.0, 700.0, 1))),
        at(1),
    );
    assert!(m.suggestion().is_none());
    // outside a, inside only b → suggest b
    m.handle(
        Msg::PositionEvent(PositionEvent::Position(pos(1200.0, 1200.0, 2))),
        at(2),
    );
    assert_eq!(m.suggestion().map(|s| &s.id), Some(&id("b")));
    // dismiss → muted; next outside fix stays muted
    m.handle(Msg::DismissSuggestion, at(3));
    m.handle(
        Msg::PositionEvent(PositionEvent::Position(pos(1250.0, 1250.0, 4))),
        at(4),
    );
    assert!(m.suggestion().is_none());
    // back inside a → unmuted; outside in nowhere → none; outside in b again → suggested
    m.handle(
        Msg::PositionEvent(PositionEvent::Position(pos(100.0, 100.0, 5))),
        at(5),
    );
    m.handle(
        Msg::PositionEvent(PositionEvent::Position(pos(9000.0, 9000.0, 6))),
        at(6),
    );
    assert!(m.suggestion().is_none());
    m.handle(
        Msg::PositionEvent(PositionEvent::Position(pos(1300.0, 1300.0, 7))),
        at(7),
    );
    assert_eq!(m.suggestion().map(|s| &s.id), Some(&id("b")));
    println!("{}", dump(&m));
    // accept = select
    let effects = m.handle(Msg::AcceptSuggestion, at(8));
    assert_eq!(effects[0], Effect::DecodeImage(id("b")));
    assert_eq!(m.selected(), &id("b"));
    assert!(m.suggestion().is_none());
}

#[test]
fn set_screenshots_dir_effect_order_and_persist() {
    let mut m = fresh();
    let effects = m.handle(Msg::SetScreenshotsDir(Some("/other".into())), at(1));
    assert_eq!(
        effects,
        vec![
            Effect::StopWatching,
            Effect::WatchScreenshots("/other".into()),
            Effect::PersistSettings(Settings {
                screenshots_dir: Some("/other".into()),
                ..Default::default()
            })
        ]
    );
    let effects = m.handle(Msg::SetScreenshotsDir(None), at(2));
    assert_eq!(effects[1], Effect::WatchScreenshots("/shots".into()));
}

#[test]
fn failures_become_deduped_notifications() {
    let mut m = fresh();
    let err = || crate::ports::ImageDecodeError {
        key: crate::domain::MapImageKey("maps/a.bc7z".into()),
        source: "boom".into(),
    };
    m.handle(
        Msg::ImageDecodeFailed {
            map: id("a"),
            error: err(),
        },
        at(1),
    );
    m.handle(
        Msg::ImageDecodeFailed {
            map: id("a"),
            error: err(),
        },
        at(2),
    );
    assert_eq!(m.notifications().len(), 1);
    assert_eq!(m.image(), &MapImage::Failed);
    let nid = m.notifications()[0].id;
    m.handle(Msg::DismissNotification(nid), at(3));
    assert!(m.notifications().is_empty());
    m.handle(
        Msg::PositionEvent(PositionEvent::Failed(WatchError::FolderMissing(
            "/shots".into(),
        ))),
        at(4),
    );
    assert!(matches!(
        m.tracking(),
        Tracking::Off(WatchError::FolderMissing(_))
    ));
    assert_eq!(m.notifications()[0].severity, Severity::Warning);
    println!("{}", dump(&m));
}

#[test]
fn freshness_is_a_query() {
    let mut m = fresh();
    assert_eq!(m.freshness(at(0)), None);
    m.handle(
        Msg::PositionEvent(PositionEvent::Position(pos(1.0, 1.0, 10))),
        at(10),
    );
    assert_eq!(m.freshness(at(100)), Some(Freshness::Live));
    assert_eq!(m.freshness(at(130)), Some(Freshness::Stale));
}
