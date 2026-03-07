use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Debug, Clone)]
pub struct LayoutInput {
    pub id: u32,
    pub value: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct LayoutRect {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

pub fn squarify(inputs: Vec<LayoutInput>, bounds: Rect) -> Vec<LayoutRect> {
    if inputs.is_empty() {
        return Vec::new();
    }

    // Sort descending by value
    let mut sorted_inputs = inputs;
    sorted_inputs.sort_by(|a, b| b.value.cmp(&a.value));

    let total_value: u64 = sorted_inputs.iter().map(|i| i.value).sum();
    let total_area = bounds.w * bounds.h;

    // Convert values to areas
    let mut items: Vec<(u32, f32)> = sorted_inputs
        .iter()
        .map(|i| (i.id, (i.value as f32 / total_value as f32) * total_area))
        .collect();

    let mut layout = Vec::new();
    let mut current_bounds = bounds;

    while !items.is_empty() {
        let mut row = Vec::new();
        let mut worst_aspect = f32::MAX;

        while let Some(&(_id, area)) = items.first() {
            let mut next_row = row.clone();
            next_row.push(area);

            let current_worst = aspect_ratio_worst(&next_row, &current_bounds);
            if current_worst <= worst_aspect {
                row.push(area);
                items.remove(0);
                worst_aspect = current_worst;
            } else {
                break;
            }
        }

        // Layout the current row
        layout.extend(layout_row(
            &row,
            &sorted_inputs,
            &mut current_bounds,
            layout.len(),
        ));
    }

    layout
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_squarify() {
        let inputs = vec![
            LayoutInput { id: 1, value: 100 },
            LayoutInput { id: 2, value: 50 },
            LayoutInput { id: 3, value: 50 },
        ];
        let bounds = Rect {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 100.0,
        };

        let layout = squarify(inputs, bounds);

        assert_eq!(layout.len(), 3);

        // Total area should be 20000
        // Input 1 (100) area = 10000
        // Input 2 (50) area = 5000
        // Input 3 (50) area = 5000

        // Input 1 is the largest, should take half the space
        assert_eq!(layout[0].w * layout[0].h, 10000.0);
        assert_eq!(
            layout[1].w * layout[1].h + layout[2].w * layout[2].h,
            10000.0
        );
    }
}

fn aspect_ratio_worst(row: &[f32], bounds: &Rect) -> f32 {
    if row.is_empty() {
        return f32::MAX;
    }

    let sum: f32 = row.iter().sum();
    let min = row.iter().fold(f32::INFINITY, |a, &b| a.min(b));
    let max = row.iter().fold(f32::NEG_INFINITY, |a, &b| a.max(b));

    let length = bounds.w.min(bounds.h);
    let s_sq = sum * sum;
    let l_sq = length * length;

    (l_sq * max / s_sq).max(s_sq / (l_sq * min))
}

fn layout_row(
    row: &[f32],
    original_inputs: &[LayoutInput],
    bounds: &mut Rect,
    processed_count: usize,
) -> Vec<LayoutRect> {
    let mut rectangles = Vec::new();
    let row_area: f32 = row.iter().sum();
    let horizontal = bounds.w >= bounds.h;

    let mut current_offset = 0.0;

    for (i, &area) in row.iter().enumerate() {
        let id = original_inputs[processed_count + i].id;
        if horizontal {
            let width = row_area / bounds.h;
            let height = area / width;
            rectangles.push(LayoutRect {
                id,
                x: bounds.x,
                y: bounds.y + current_offset,
                w: width,
                h: height,
            });
            current_offset += height;
        } else {
            let height = row_area / bounds.w;
            let width = area / height;
            rectangles.push(LayoutRect {
                id,
                x: bounds.x + current_offset,
                y: bounds.y,
                w: width,
                h: height,
            });
            current_offset += width;
        }
    }

    // Update remaining bounds
    if horizontal {
        let width = row_area / bounds.h;
        bounds.x += width;
        bounds.w -= width;
    } else {
        let height = row_area / bounds.w;
        bounds.y += height;
        bounds.h -= height;
    }

    rectangles
}
