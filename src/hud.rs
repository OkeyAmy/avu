use crate::domain::CockpitMode;

#[derive(Debug, Clone)]
pub struct HudFrame {
    pub lines: Vec<String>,
}

pub fn render_hud(mode: &CockpitMode, width: usize, height: usize, activity: u8) -> HudFrame {
    let width = width.max(24);
    let height = height.max(11);
    let mut grid = vec![vec![' '; width]; height];
    let cx = (width as f32 - 1.0) / 2.0;
    let cy = (height as f32 - 1.0) / 2.0;
    let radius = width.min(height * 2) as f32 / 3.2;
    let pulse = 1.0 + (activity as f32 / 100.0) * 0.18;

    for (y, row) in grid.iter_mut().enumerate().take(height) {
        for (x, cell) in row.iter_mut().enumerate().take(width) {
            let dx = (x as f32 - cx) / 2.0;
            let dy = y as f32 - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            let angle = dy.atan2(dx);
            let wave = (angle * 6.0).sin() * pulse;
            let outer = radius + wave;
            let inner = radius * 0.58;
            let core = radius * 0.24;
            let tick = ((angle.to_degrees().abs() as i32) % 30) < 4;

            *cell = if (dist - outer).abs() < 0.42 {
                if tick { '╋' } else { '◌' }
            } else if (dist - inner).abs() < 0.35 {
                '·'
            } else if dist < core {
                match mode {
                    CockpitMode::ApprovalNeeded => '◆',
                    CockpitMode::Disconnected | CockpitMode::Error => '×',
                    CockpitMode::Speaking => '◍',
                    _ => '◉',
                }
            } else {
                ' '
            };
        }
    }

    let label = format!(" {} ", mode.label());
    write_center(&mut grid, height / 2, &label);
    write_center(
        &mut grid,
        height.saturating_sub(2),
        "ACP/GATEWAY EVENT RADAR",
    );

    HudFrame {
        lines: grid
            .into_iter()
            .map(|row| row.into_iter().collect())
            .collect(),
    }
}

fn write_center(grid: &mut [Vec<char>], row: usize, text: &str) {
    if row >= grid.len() || grid[row].is_empty() {
        return;
    }
    let width = grid[row].len();
    let start = width.saturating_sub(text.chars().count()) / 2;
    for (offset, ch) in text.chars().enumerate() {
        if start + offset < width {
            grid[row][start + offset] = ch;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hud_contains_mode_and_radar_language() {
        let hud = render_hud(&CockpitMode::ApprovalNeeded, 42, 15, 80);
        let output = hud.lines.join("\n");
        assert!(output.contains("APPROVAL"));
        assert!(output.contains("EVENT RADAR"));
        assert!(output.contains('◆'));
    }
}
