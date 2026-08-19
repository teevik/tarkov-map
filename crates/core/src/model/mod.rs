//! Application model: sync `handle(msg, now) -> Vec<Effect>` is the only mutation path (ADR-0002).
//! This skeleton carries a *subset* of the decided messages/effects — enough to feel the shape.

use std::collections::{BTreeSet, VecDeque};
use std::path::PathBuf;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::domain::{Freshness, ImagePoint, Map, MapCatalog, MapId, PlayerPosition};
use crate::ports::{ImageDecodeError, MapImageData, PositionEvent, SourceKind, WatchError};

#[cfg(test)]
mod tests;

pub const TRAIL_CAPACITY: usize = 20;

// ---------------------------------------------------------------- state

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum OverlayKind {
    Labels,
    Spawns,
    PmcExtracts,
    ScavExtracts,
    SharedExtracts,
    PlayerMarker,
    Trail,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub selected_map: Option<MapId>,
    pub hidden_overlays: BTreeSet<OverlayKind>,
    pub screenshots_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    /// 1 = fit whole Map on screen … 10
    pub zoom: f64,
    pub center: ImagePoint,
}

impl Viewport {
    fn fit(map: &Map) -> Self {
        Self {
            zoom: 1.0,
            center: (map.image_size / 2.0).to_vector().to_point(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum MapImage {
    Loading { since: SystemTime },
    Ready { since: SystemTime },
    Failed,
}

#[derive(Debug)]
pub enum Tracking {
    Off(WatchError),
    Waiting(SourceKind),
    Positioned(SourceKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct NotificationId(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Notification {
    pub id: NotificationId,
    pub severity: Severity,
    pub text: String,
    pub raised_at: SystemTime,
}

#[derive(Debug)]
pub struct Model {
    catalog: MapCatalog,
    selected: MapId,
    viewport: Viewport,
    image: MapImage,
    outgoing: Option<MapId>,
    /// back() = current Player Position, the rest = Trail
    positions: VecDeque<PlayerPosition>,
    tracking: Tracking,
    suggestion: Option<MapId>,
    suggestion_muted: bool,
    settings: Settings,
    default_screenshots_dir: Option<PathBuf>,
    notifications: Vec<Notification>,
    next_notification: u64,
}

// ---------------------------------------------------------------- messages & effects

#[derive(Debug)]
pub enum Msg {
    SelectMap(MapId),
    SelectMapByIndex(usize),
    PositionEvent(PositionEvent),
    ImageDecoded { map: MapId, image: MapImageData },
    ImageDecodeFailed { map: MapId, error: ImageDecodeError },
    AcceptSuggestion,
    DismissSuggestion,
    SetScreenshotsDir(Option<PathBuf>),
    DismissNotification(NotificationId),
}

#[derive(Debug, PartialEq)]
pub enum Effect {
    DecodeImage(MapId),
    /// App-intercepted (GPU upload).
    PresentImage {
        map: MapId,
        image: MapImageData,
    },
    WatchScreenshots(PathBuf),
    StopWatching,
    PersistSettings(Settings),
}

// ---------------------------------------------------------------- behaviour

impl Model {
    pub fn new(
        catalog: MapCatalog,
        settings: Settings,
        default_screenshots_dir: Option<PathBuf>,
        now: SystemTime,
    ) -> (Self, Vec<Effect>) {
        let selected = settings
            .selected_map
            .as_ref()
            .and_then(|id| catalog.get(id))
            .unwrap_or_else(|| catalog.first())
            .id
            .clone();
        let viewport = Viewport::fit(catalog.get(&selected).unwrap());
        let mut model = Self {
            catalog,
            selected,
            viewport,
            image: MapImage::Loading { since: now },
            outgoing: None,
            positions: VecDeque::with_capacity(TRAIL_CAPACITY),
            tracking: Tracking::Off(WatchError::NoDocumentsFolder),
            suggestion: None,
            suggestion_muted: false,
            settings,
            default_screenshots_dir,
            notifications: vec![],
            next_notification: 0,
        };
        let mut effects = vec![Effect::DecodeImage(model.selected.clone())];
        effects.extend(model.start_watching());
        (model, effects)
    }

    /// The only mutation path.
    pub fn handle(&mut self, msg: Msg, now: SystemTime) -> Vec<Effect> {
        match msg {
            Msg::SelectMap(id) => self.select(id, now),
            Msg::SelectMapByIndex(i) => match self.catalog.by_index(i).map(|m| m.id.clone()) {
                Some(id) => self.select(id, now),
                None => vec![],
            },
            Msg::PositionEvent(ev) => {
                self.position_event(ev, now);
                vec![]
            }
            Msg::ImageDecoded { map, image } => {
                if map != self.selected {
                    return vec![]; // stale decode for a map no longer selected (ADR-0002)
                }
                self.image = MapImage::Ready { since: now };
                vec![Effect::PresentImage { map, image }]
            }
            Msg::ImageDecodeFailed { map, error } => {
                if map == self.selected {
                    self.image = MapImage::Failed;
                }
                self.notify(Severity::Error, error.to_string(), now);
                vec![]
            }
            Msg::AcceptSuggestion => match self.suggestion.take() {
                Some(id) => self.select(id, now),
                None => vec![],
            },
            Msg::DismissSuggestion => {
                self.suggestion = None;
                self.suggestion_muted = true;
                vec![]
            }
            Msg::SetScreenshotsDir(dir) => {
                self.settings.screenshots_dir = dir;
                let mut effects = vec![Effect::StopWatching];
                effects.extend(self.start_watching());
                effects.push(Effect::PersistSettings(self.settings.clone()));
                effects
            }
            Msg::DismissNotification(id) => {
                self.notifications.retain(|n| n.id != id);
                vec![]
            }
        }
    }

    fn select(&mut self, id: MapId, now: SystemTime) -> Vec<Effect> {
        if id == self.selected || self.catalog.get(&id).is_none() {
            return vec![];
        }
        self.outgoing = Some(std::mem::replace(&mut self.selected, id));
        self.viewport = Viewport::fit(self.selected_map());
        self.image = MapImage::Loading { since: now };
        // Trail belongs to one Map: keep only the newest Player Position
        if let Some(newest) = self.positions.pop_back() {
            self.positions.clear();
            self.positions.push_back(newest);
        }
        self.suggestion = None;
        self.suggestion_muted = false;
        self.reevaluate_suggestion();
        self.settings.selected_map = Some(self.selected.clone());
        vec![
            Effect::DecodeImage(self.selected.clone()),
            Effect::PersistSettings(self.settings.clone()),
        ]
    }

    fn position_event(&mut self, ev: PositionEvent, now: SystemTime) {
        match ev {
            PositionEvent::Started(kind) => {
                self.positions.clear();
                self.tracking = Tracking::Waiting(kind);
            }
            PositionEvent::Failed(err) => {
                self.notify(Severity::Warning, err.to_string(), now);
                self.tracking = Tracking::Off(err);
            }
            PositionEvent::Position(p) => {
                if let Some(cur) = self.positions.back()
                    && (*cur == p || p.taken_at < cur.taken_at)
                {
                    return;
                }
                if self.positions.len() == TRAIL_CAPACITY {
                    self.positions.pop_front();
                }
                self.positions.push_back(p);
                if let Tracking::Waiting(kind) = self.tracking {
                    self.tracking = Tracking::Positioned(kind);
                }
                self.reevaluate_suggestion();
            }
        }
    }

    fn reevaluate_suggestion(&mut self) {
        let Some(p) = self.positions.back() else {
            return;
        };
        if self.selected_map().contains(p.ground) {
            self.suggestion = None;
            self.suggestion_muted = false;
            return;
        }
        let mut candidates = self.catalog.containing(p.ground, &self.selected);
        self.suggestion = match (candidates.next(), candidates.next()) {
            (Some(only), None) if !self.suggestion_muted => Some(only.id.clone()),
            _ => None,
        };
    }

    fn start_watching(&mut self) -> Option<Effect> {
        match self
            .settings
            .screenshots_dir
            .clone()
            .or_else(|| self.default_screenshots_dir.clone())
        {
            Some(dir) => Some(Effect::WatchScreenshots(dir)),
            None => {
                self.tracking = Tracking::Off(WatchError::NoDocumentsFolder);
                None
            }
        }
    }

    fn notify(&mut self, severity: Severity, text: String, now: SystemTime) {
        if self.notifications.iter().any(|n| n.text == text) {
            return;
        }
        let id = NotificationId(self.next_notification);
        self.next_notification += 1;
        self.notifications.push(Notification {
            id,
            severity,
            text,
            raised_at: now,
        });
    }

    // ------------------------------------------------------------ queries (borrowed, per frame)

    pub fn selected_map(&self) -> &Map {
        self.catalog
            .get(&self.selected)
            .expect("selected is always a catalogue id")
    }
    pub fn selected(&self) -> &MapId {
        &self.selected
    }
    pub fn catalog(&self) -> &MapCatalog {
        &self.catalog
    }
    pub fn viewport(&self) -> Viewport {
        self.viewport
    }
    pub fn image(&self) -> &MapImage {
        &self.image
    }
    pub fn outgoing(&self) -> Option<&MapId> {
        self.outgoing.as_ref()
    }
    pub fn current_position(&self) -> Option<&PlayerPosition> {
        self.positions.back()
    }
    /// Trail = everything but the newest, oldest first.
    pub fn trail(&self) -> impl Iterator<Item = &PlayerPosition> {
        self.positions
            .iter()
            .take(self.positions.len().saturating_sub(1))
    }
    pub fn freshness(&self, now: SystemTime) -> Option<Freshness> {
        self.current_position().map(|p| p.freshness(now))
    }
    pub fn tracking(&self) -> &Tracking {
        &self.tracking
    }
    pub fn suggestion(&self) -> Option<&Map> {
        self.suggestion.as_ref().and_then(|id| self.catalog.get(id))
    }
    pub fn settings(&self) -> &Settings {
        &self.settings
    }
    pub fn notifications(&self) -> &[Notification] {
        &self.notifications
    }
}
