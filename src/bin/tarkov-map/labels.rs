//! Shared placement and drawing for Labels contributed by visible Overlays.

use eframe::egui;

const LABEL_PADDING: f32 = 2.0;

/// Fixed priority tiers for every labelled Overlay.
#[allow(dead_code)] // Later Overlay tickets use the reserved tiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LabelKind {
    Extract,
    Transit,
    BtrStop,
    Switch,
    BossSpawn,
    PlaceName,
}
/// A measured Label ready for the shared placement pass.
#[derive(Debug, Clone, PartialEq)]
pub struct LabelCandidate {
    pub kind: LabelKind,
    pub within_kind_priority: f64,
    pub source_order: usize,
    pub text: String,
    pub font: egui::FontId,
    pub color: egui::Color32,
    pub outline: egui::Color32,
    pub anchor: egui::Pos2,
    pub align: egui::Align2,
    pub measured: egui::Vec2,
}

impl LabelCandidate {
    fn bounds(&self) -> egui::Rect {
        self.align.anchor_size(self.anchor, self.measured)
    }
}

/// Sorts and greedily places Labels that do not overlap an earlier Label.
pub fn place(candidates: Vec<LabelCandidate>) -> Vec<LabelCandidate> {
    let mut candidates = candidates;
    candidates.sort_by(|left, right| {
        left.kind
            .cmp(&right.kind)
            .then_with(|| {
                right
                    .within_kind_priority
                    .total_cmp(&left.within_kind_priority)
            })
            .then_with(|| left.source_order.cmp(&right.source_order))
    });

    let mut occupied = Vec::with_capacity(candidates.len());
    candidates
        .into_iter()
        .filter(|candidate| {
            let bounds = candidate.bounds().expand(LABEL_PADDING);
            if occupied
                .iter()
                .any(|placed: &egui::Rect| placed.intersects(bounds))
            {
                false
            } else {
                occupied.push(bounds);
                true
            }
        })
        .collect()
}

/// Draws already-placed Labels, skipping survivors outside the current clip rectangle.
pub fn draw<'a>(painter: &egui::Painter, labels: impl IntoIterator<Item = &'a LabelCandidate>) {
    for label in labels {
        if !painter.clip_rect().intersects(label.bounds()) {
            continue;
        }

        for offset in [
            egui::vec2(-1.0, 0.0),
            egui::vec2(1.0, 0.0),
            egui::vec2(0.0, -1.0),
            egui::vec2(0.0, 1.0),
        ] {
            painter.text(
                label.anchor + offset,
                label.align,
                &label.text,
                label.font.clone(),
                label.outline,
            );
        }
        painter.text(
            label.anchor,
            label.align,
            &label.text,
            label.font.clone(),
            label.color,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(kind: LabelKind, seq: usize) -> LabelCandidate {
        LabelCandidate {
            kind,
            within_kind_priority: 0.0,
            source_order: seq,
            text: seq.to_string(),
            font: egui::FontId::default(),
            color: egui::Color32::WHITE,
            outline: egui::Color32::BLACK,
            anchor: egui::pos2(0.0, 0.0),
            align: egui::Align2::LEFT_TOP,
            measured: egui::vec2(10.0, 10.0),
        }
    }

    fn ranked_candidate(kind: LabelKind, rank: f64, seq: usize) -> LabelCandidate {
        LabelCandidate {
            within_kind_priority: rank,
            ..candidate(kind, seq)
        }
    }

    fn positioned_candidate(seq: usize, x: f32) -> LabelCandidate {
        LabelCandidate {
            anchor: egui::pos2(x, 0.0),
            ..candidate(LabelKind::Extract, seq)
        }
    }

    #[test]
    fn kind_tier_decides_which_overlapping_label_survives() {
        let placed = place(vec![
            candidate(LabelKind::PlaceName, 0),
            candidate(LabelKind::BossSpawn, 1),
            candidate(LabelKind::Switch, 2),
            candidate(LabelKind::BtrStop, 3),
            candidate(LabelKind::Transit, 4),
            candidate(LabelKind::Extract, 5),
        ]);

        assert_eq!(
            placed
                .iter()
                .map(|label| label.source_order)
                .collect::<Vec<_>>(),
            [5]
        );
    }

    #[test]
    fn within_kind_rank_descends_before_data_order_breaks_ties() {
        let place_names = place(vec![
            ranked_candidate(LabelKind::PlaceName, 20.0, 0),
            ranked_candidate(LabelKind::PlaceName, 40.0, 1),
        ]);
        let extracts = place(vec![
            ranked_candidate(LabelKind::Extract, 0.0, 10),
            ranked_candidate(LabelKind::Extract, 0.0, 2),
        ]);

        assert_eq!(place_names[0].source_order, 1);
        assert_eq!(extracts[0].source_order, 2);
    }

    #[test]
    fn two_pixel_padding_sets_the_label_collision_gap() {
        let three_pixel_gap = place(vec![
            positioned_candidate(0, 0.0),
            positioned_candidate(1, 13.0),
        ]);
        let five_pixel_gap = place(vec![
            positioned_candidate(0, 0.0),
            positioned_candidate(1, 15.0),
        ]);

        assert_eq!(
            three_pixel_gap
                .iter()
                .map(|label| label.source_order)
                .collect::<Vec<_>>(),
            [0]
        );
        assert_eq!(
            five_pixel_gap
                .iter()
                .map(|label| label.source_order)
                .collect::<Vec<_>>(),
            [0, 1]
        );
    }

    #[test]
    fn a_multiline_label_is_claimed_as_one_whole_box() {
        let mut multiline = candidate(LabelKind::Switch, 0);
        multiline.text = "Power\nElevator".to_owned();
        multiline.measured = egui::vec2(20.0, 30.0);
        let mut overlaps_second_line = candidate(LabelKind::Switch, 1);
        overlaps_second_line.anchor = egui::pos2(0.0, 22.0);

        let placed = place(vec![multiline, overlaps_second_line]);

        assert_eq!(
            placed
                .iter()
                .map(|label| label.source_order)
                .collect::<Vec<_>>(),
            [0]
        );
    }

    #[test]
    fn translating_every_anchor_keeps_the_same_survivors() {
        let candidates = vec![
            positioned_candidate(0, 0.0),
            positioned_candidate(1, 13.0),
            positioned_candidate(2, 30.0),
        ];
        let translated = candidates
            .iter()
            .cloned()
            .map(|mut label| {
                label.anchor += egui::vec2(417.0, -93.0);
                label
            })
            .collect();

        let survivors = |labels: Vec<LabelCandidate>| {
            place(labels)
                .into_iter()
                .map(|label| label.source_order)
                .collect::<Vec<_>>()
        };

        assert_eq!(survivors(candidates), survivors(translated));
    }

    #[test]
    fn placement_is_deterministic() {
        let candidates = vec![
            ranked_candidate(LabelKind::PlaceName, 80.0, 3),
            positioned_candidate(2, 30.0),
            positioned_candidate(1, 13.0),
            positioned_candidate(0, 0.0),
        ];

        assert_eq!(place(candidates.clone()), place(candidates));
    }
}
