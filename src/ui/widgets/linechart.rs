use ratatui::symbols;
use ratatui::text::Line;
use ratatui::widgets::{Axis, Chart, Dataset, GraphType};

use crate::analytics::model_stats::ModelChartData;
use crate::ui::theme::Theme;

pub fn build_chart<'a>(chart: &'a ModelChartData, theme: &'a Theme) -> Chart<'a> {
    let datasets = chart
        .series
        .iter()
        .map(|series| {
            Dataset::default()
                .name(series.model_id.clone())
                .marker(symbols::Marker::Braille)
                .graph_type(GraphType::Line)
                .style(
                    ratatui::style::Style::default().fg(theme.series_color(series.palette_index)),
                )
                .data(&series.points)
        })
        .collect::<Vec<_>>();

    let x_axis = Axis::default()
        .bounds(chart.x_bounds)
        .style(ratatui::style::Style::default().fg(theme.muted))
        .labels(
            chart
                .x_labels
                .iter()
                .map(|label| Line::from(label.clone()))
                .collect::<Vec<_>>(),
        )
        .labels_alignment(ratatui::layout::Alignment::Center);

    let y_axis = Axis::default()
        .bounds(chart.y_bounds)
        .style(ratatui::style::Style::default().fg(theme.muted))
        .labels(
            chart
                .y_labels
                .iter()
                .map(|label| Line::from(label.clone()))
                .collect::<Vec<_>>(),
        );

    Chart::new(datasets)
        .legend_position(None)
        .x_axis(x_axis)
        .y_axis(y_axis)
}
