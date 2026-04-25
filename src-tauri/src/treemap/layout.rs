use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum LayoutRectKind {
    Node,
    Overflow,
}

#[derive(Debug, Clone)]
pub struct LayoutInput {
    pub id: Option<u32>,
    pub label: String,
    pub kind: LayoutRectKind,
    pub value: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LayoutRect {
    pub id: Option<u32>,
    pub kind: LayoutRectKind,
    pub label: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

pub fn squarify(inputs: Vec<LayoutInput>, bounds: Rect) -> Vec<LayoutRect> {
    if inputs.is_empty() || bounds.w <= 0.0 || bounds.h <= 0.0 {
        return Vec::new();
    }

    let mut sorted_inputs = inputs;
    sorted_inputs
        .sort_unstable_by(|a, b| b.value.cmp(&a.value).then_with(|| a.label.cmp(&b.label)));

    let total_value: u64 = sorted_inputs.iter().map(|item| item.value).sum();
    if total_value == 0 {
        return Vec::new();
    }

    let total_area = bounds.w * bounds.h;
    let item_areas: Vec<f32> = sorted_inputs
        .iter()
        .map(|item| (item.value as f32 / total_value as f32) * total_area)
        .collect();

    let mut layout = Vec::with_capacity(sorted_inputs.len());
    let mut current_bounds = bounds;
    let mut processed = 0usize;

    while processed < sorted_inputs.len() && current_bounds.w > 0.0 && current_bounds.h > 0.0 {
        let mut row_len = 0usize;
        let mut row_sum = 0.0f32;
        let mut row_min = f32::INFINITY;
        let mut row_max = 0.0f32;
        let mut worst_aspect = f32::MAX;

        while processed + row_len < item_areas.len() {
            let area = item_areas[processed + row_len];
            let candidate_sum = row_sum + area;
            let candidate_min = row_min.min(area);
            let candidate_max = row_max.max(area);
            let candidate_worst =
                aspect_ratio_worst(candidate_sum, candidate_min, candidate_max, &current_bounds);

            if row_len == 0 || candidate_worst <= worst_aspect {
                row_len += 1;
                row_sum = candidate_sum;
                row_min = candidate_min;
                row_max = candidate_max;
                worst_aspect = candidate_worst;
            } else {
                break;
            }
        }

        if row_len == 0 {
            break;
        }

        layout.extend(layout_row(
            &item_areas[processed..processed + row_len],
            &sorted_inputs[processed..processed + row_len],
            &mut current_bounds,
        ));
        processed += row_len;
    }

    layout
}

fn aspect_ratio_worst(sum: f32, min: f32, max: f32, bounds: &Rect) -> f32 {
    if sum <= 0.0 || min <= 0.0 || max <= 0.0 {
        return f32::MAX;
    }

    let length = bounds.w.min(bounds.h);
    if length <= 0.0 {
        return f32::MAX;
    }

    let sum_sq = sum * sum;
    let length_sq = length * length;

    (length_sq * max / sum_sq).max(sum_sq / (length_sq * min))
}

fn layout_row(row_areas: &[f32], row_inputs: &[LayoutInput], bounds: &mut Rect) -> Vec<LayoutRect> {
    let mut rectangles = Vec::with_capacity(row_inputs.len());
    let row_area: f32 = row_areas.iter().sum();
    let horizontal = bounds.w >= bounds.h;
    let strip_size = if horizontal {
        row_area / bounds.h
    } else {
        row_area / bounds.w
    };

    let mut current_offset = 0.0f32;

    for (area, input) in row_areas.iter().zip(row_inputs.iter()) {
        if horizontal {
            let height = area / strip_size;
            rectangles.push(LayoutRect {
                id: input.id,
                kind: input.kind,
                label: input.label.clone(),
                x: bounds.x,
                y: bounds.y + current_offset,
                w: strip_size,
                h: height,
            });
            current_offset += height;
        } else {
            let width = area / strip_size;
            rectangles.push(LayoutRect {
                id: input.id,
                kind: input.kind,
                label: input.label.clone(),
                x: bounds.x + current_offset,
                y: bounds.y,
                w: width,
                h: strip_size,
            });
            current_offset += width;
        }
    }

    if horizontal {
        bounds.x += strip_size;
        bounds.w -= strip_size;
    } else {
        bounds.y += strip_size;
        bounds.h -= strip_size;
    }

    rectangles
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_squarify_preserves_total_area() {
        let inputs = vec![
            LayoutInput {
                id: Some(1),
                label: "one".to_string(),
                kind: LayoutRectKind::Node,
                value: 100,
            },
            LayoutInput {
                id: Some(2),
                label: "two".to_string(),
                kind: LayoutRectKind::Node,
                value: 50,
            },
            LayoutInput {
                id: Some(3),
                label: "three".to_string(),
                kind: LayoutRectKind::Node,
                value: 50,
            },
        ];
        let bounds = Rect {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 100.0,
        };

        let layout = squarify(inputs, bounds);
        let total_area: f32 = layout.iter().map(|rect| rect.w * rect.h).sum();

        assert_eq!(layout.len(), 3);
        assert!((total_area - 20_000.0).abs() < 0.1);
    }

    #[test]
    fn overflow_tiles_keep_their_kind_and_label() {
        let inputs = vec![
            LayoutInput {
                id: Some(10),
                label: "A".to_string(),
                kind: LayoutRectKind::Node,
                value: 60,
            },
            LayoutInput {
                id: None,
                label: "Other".to_string(),
                kind: LayoutRectKind::Overflow,
                value: 40,
            },
        ];

        let layout = squarify(
            inputs,
            Rect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 100.0,
            },
        );

        assert_eq!(layout.len(), 2);
        assert_eq!(layout[1].kind, LayoutRectKind::Overflow);
        assert_eq!(layout[1].label, "Other");
        assert_eq!(layout[1].id, None);
    }
}
