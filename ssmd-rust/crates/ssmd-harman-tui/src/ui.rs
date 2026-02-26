use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, TableState},
    Frame,
};
use rust_decimal::Decimal;

use crate::app::{App, OrderField, Tab};
use crate::types::OrderState;

/// Render the full TUI frame.
pub fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // status bar
            Constraint::Min(0),   // main content
            Constraint::Length(1), // keybinding hints
        ])
        .split(f.area());

    draw_status_bar(f, app, chunks[0]);

    match app.active_tab {
        Tab::Orders => draw_orders(f, app, chunks[1]),
        Tab::Risk => draw_risk(f, app, chunks[1]),
        Tab::Positions => draw_positions(f, app, chunks[1]),
        Tab::MarketData => draw_market_data(f, app, chunks[1]),
        Tab::Help => draw_help(f, app, chunks[1]),
    }

    draw_hints(f, app, chunks[2]);

    // Overlay dialogs
    if app.order_form.is_some() {
        draw_order_form(f, app);
    } else if app.show_confirmation {
        draw_confirmation(f);
    }
}

fn draw_status_bar(f: &mut Frame, app: &App, area: Rect) {
    let mut spans = Vec::new();

    // Connection status
    match &app.last_error {
        None => {
            spans.push(Span::styled(" Connected ", Style::default().fg(Color::Green)));
        }
        Some(err) => {
            let msg = if err.len() > 40 {
                format!(" ERR: {}... ", &err[..40])
            } else {
                format!(" ERR: {} ", err)
            };
            spans.push(Span::styled(msg, Style::default().fg(Color::Red)));
        }
    }

    spans.push(Span::raw(" | "));

    // Last poll
    match app.last_poll {
        Some(t) => {
            let ago = t.elapsed().as_secs();
            spans.push(Span::raw(format!("polled {}s ago", ago)));
        }
        None => {
            spans.push(Span::styled("no poll yet", Style::default().fg(Color::DarkGray)));
        }
    }

    spans.push(Span::raw(format!(" ({}s)", app.poll_interval.as_secs())));
    spans.push(Span::raw(" | "));

    // Active filter
    let filter_label = match app.state_filter {
        None => "All".to_string(),
        Some(s) => format!("{}", s),
    };
    spans.push(Span::styled(
        format!("filter: {}", filter_label),
        Style::default().fg(Color::Cyan),
    ));

    // Tab indicator
    spans.push(Span::raw(" | "));
    let tab_label = match app.active_tab {
        Tab::Orders => "Orders",
        Tab::Risk => "Risk",
        Tab::Positions => "Positions",
        Tab::MarketData => "Market Data",
        Tab::Help => "Help",
    };
    spans.push(Span::styled(
        format!("[{}]", tab_label),
        Style::default().fg(Color::Yellow),
    ));

    // Last action result
    if let Some(ref result) = app.last_action_result {
        spans.push(Span::raw(" | "));
        spans.push(Span::styled(result.clone(), Style::default().fg(Color::Magenta)));
    }

    let status = Paragraph::new(Line::from(spans))
        .style(Style::default().bg(Color::DarkGray).fg(Color::White));
    f.render_widget(status, area);
}

fn state_color(state: OrderState) -> Color {
    match state {
        OrderState::Filled => Color::Green,
        OrderState::Rejected | OrderState::Cancelled => Color::Red,
        OrderState::Pending | OrderState::PendingCancel | OrderState::PendingAmend | OrderState::PendingDecrease => Color::Yellow,
        OrderState::Acknowledged | OrderState::Submitted => Color::LightGreen,
        OrderState::PartiallyFilled => Color::Magenta,
        OrderState::Expired => Color::DarkGray,
    }
}

fn draw_orders(f: &mut Frame, app: &App, area: Rect) {
    let filtered = app.filtered_orders();

    let header = Row::new(vec![
        Cell::from("ID"),
        Cell::from("Ticker"),
        Cell::from("Side"),
        Cell::from("Action"),
        Cell::from("Qty"),
        Cell::from("Price"),
        Cell::from("Filled"),
        Cell::from("State"),
        Cell::from("Updated"),
    ])
    .style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );

    let rows: Vec<Row> = filtered
        .iter()
        .map(|order| {
            let color = state_color(order.state);
            Row::new(vec![
                Cell::from(order.id.to_string()),
                Cell::from(order.ticker.clone()),
                Cell::from(order.side.to_string()),
                Cell::from(order.action.to_string()),
                Cell::from(order.quantity.to_string()),
                Cell::from(format!("${}", order.price_dollars)),
                Cell::from(order.filled_quantity.to_string()),
                Cell::from(order.state.to_string()).style(Style::default().fg(color)),
                Cell::from(order.updated_at.format("%H:%M:%S").to_string()),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(8),
        Constraint::Min(16),
        Constraint::Length(5),
        Constraint::Length(6),
        Constraint::Length(8),
        Constraint::Length(10),
        Constraint::Length(8),
        Constraint::Length(16),
        Constraint::Length(10),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" Orders ({}) ", filtered.len())),
        )
        .row_highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        );

    let mut state = TableState::default();
    if !filtered.is_empty() {
        state.select(Some(app.selected_index));
    }

    f.render_stateful_widget(table, area, &mut state);
}

fn draw_risk(f: &mut Frame, app: &App, area: Rect) {
    let content = match &app.risk {
        Some(risk) => {
            let utilization = if risk.max_notional > Decimal::ZERO {
                let pct = (risk.open_notional * Decimal::from(100)) / risk.max_notional;
                format!("{}%", pct.round_dp(1))
            } else {
                "N/A".to_string()
            };

            vec![
                Line::from(""),
                Line::from(vec![
                    Span::styled("  Max Notional:       ", Style::default().fg(Color::Cyan)),
                    Span::raw(format!("${}", risk.max_notional)),
                ]),
                Line::from(vec![
                    Span::styled("  Open Notional:      ", Style::default().fg(Color::Cyan)),
                    Span::raw(format!("${}", risk.open_notional)),
                ]),
                Line::from(vec![
                    Span::styled("  Available Notional: ", Style::default().fg(Color::Cyan)),
                    Span::raw(format!("${}", risk.available_notional)),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::styled("  Utilization:        ", Style::default().fg(Color::Yellow)),
                    Span::styled(utilization, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                ]),
            ]
        }
        None => {
            vec![
                Line::from(""),
                Line::from(Span::styled(
                    "  No risk data available yet...",
                    Style::default().fg(Color::DarkGray),
                )),
            ]
        }
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Risk ");

    let paragraph = Paragraph::new(content).block(block);
    f.render_widget(paragraph, area);
}

fn draw_help(f: &mut Frame, app: &App, area: Rect) {
    let mut rows = vec![
        Row::new(vec!["q", "Quit"]),
        Row::new(vec!["j / Down", "Select next row"]),
        Row::new(vec!["k / Up", "Select previous row"]),
        Row::new(vec!["Tab", "Cycle tabs"]),
        Row::new(vec!["?", "Toggle help"]),
        Row::new(vec!["p", "Trigger pump"]),
        Row::new(vec!["r", "Trigger reconcile"]),
        Row::new(vec!["n", "New order"]),
        Row::new(vec!["c", "Cancel selected order"]),
        Row::new(vec!["x", "Mass cancel (with confirmation)"]),
        Row::new(vec!["1", "Filter: All"]),
        Row::new(vec!["2", "Filter: Pending"]),
        Row::new(vec!["3", "Filter: Acknowledged"]),
        Row::new(vec!["4", "Filter: Filled"]),
        Row::new(vec!["5", "Filter: Cancelled"]),
    ];
    rows.push(Row::new(vec!["", ""]));
    rows.push(Row::new(vec!["", "Positions tab shows exchange positions"]));
    rows.push(Row::new(vec!["", "with live market prices from snapshots"]));
    if app.has_market_data() {
        rows.push(Row::new(vec!["f", "Cycle feed (Market Data tab)"]));
    }

    let widths = [Constraint::Length(14), Constraint::Min(30)];

    let table = Table::new(rows, widths)
        .header(
            Row::new(vec!["Key", "Action"]).style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Keybindings "),
        );

    f.render_widget(table, area);
}

fn format_price(val: Option<f64>) -> String {
    match val {
        Some(v) => format!("{:.4}", v),
        None => "—".to_string(),
    }
}

fn draw_positions(f: &mut Frame, app: &App, area: Rect) {
    if app.positions.is_empty() {
        let msg = Paragraph::new("  No exchange positions")
            .style(Style::default().fg(Color::DarkGray))
            .block(Block::default().borders(Borders::ALL).title(" Positions "));
        f.render_widget(msg, area);
        return;
    }

    let header = Row::new(vec![
        Cell::from("Ticker"),
        Cell::from("Side"),
        Cell::from("Qty"),
        Cell::from("Mkt Value"),
        Cell::from("Yes Bid"),
        Cell::from("Yes Ask"),
        Cell::from("Last"),
    ])
    .style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );

    let rows: Vec<Row> = app
        .positions
        .iter()
        .map(|pos| {
            // Look up snapshot for this ticker
            let snap = app.snapshots.iter().find(|s| s.ticker == pos.ticker);
            let side_color = match pos.side {
                crate::types::Side::Yes => Color::Green,
                crate::types::Side::No => Color::Red,
            };
            Row::new(vec![
                Cell::from(pos.ticker.clone()),
                Cell::from(pos.side.to_string()).style(Style::default().fg(side_color)),
                Cell::from(pos.quantity.to_string()),
                Cell::from(format!("${}", pos.market_value_dollars)),
                Cell::from(format_price(snap.and_then(|s| s.yes_bid))),
                Cell::from(format_price(snap.and_then(|s| s.yes_ask))),
                Cell::from(format_price(snap.and_then(|s| s.last_price))),
            ])
        })
        .collect();

    let widths = [
        Constraint::Min(20),
        Constraint::Length(6),
        Constraint::Length(10),
        Constraint::Length(12),
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Length(10),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" Positions ({}) ", app.positions.len())),
        )
        .row_highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        );

    let mut state = TableState::default();
    if !app.positions.is_empty() {
        state.select(Some(app.pos_selected));
    }

    f.render_stateful_widget(table, area, &mut state);
}

fn draw_market_data(f: &mut Frame, app: &App, area: Rect) {
    if app.data_client.is_none() {
        let msg = Paragraph::new("  Market data unavailable — set SSMD_API_URL and SSMD_DATA_API_KEY")
            .style(Style::default().fg(Color::DarkGray))
            .block(Block::default().borders(Borders::ALL).title(" Market Data "));
        f.render_widget(msg, area);
        return;
    }

    let feed = app.market_feed.as_str();

    let (header, widths, rows): (Row, Vec<Constraint>, Vec<Row>) = match feed {
        "kalshi" => {
            let header = Row::new(vec!["Ticker", "Yes Bid", "Yes Ask", "No Bid", "No Ask", "Last"])
                .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
            let widths = vec![
                Constraint::Min(20),
                Constraint::Length(10),
                Constraint::Length(10),
                Constraint::Length(10),
                Constraint::Length(10),
                Constraint::Length(10),
            ];
            let rows: Vec<Row> = app.snapshots.iter().map(|s| {
                let color = match s.yes_bid {
                    Some(v) if v > 0.50 => Color::Green,
                    Some(_) => Color::Red,
                    None => Color::White,
                };
                Row::new(vec![
                    Cell::from(s.ticker.clone()),
                    Cell::from(format_price(s.yes_bid)),
                    Cell::from(format_price(s.yes_ask)),
                    Cell::from(format_price(s.no_bid)),
                    Cell::from(format_price(s.no_ask)),
                    Cell::from(format_price(s.last_price)),
                ]).style(Style::default().fg(color))
            }).collect();
            (header, widths, rows)
        }
        "kraken-futures" => {
            let header = Row::new(vec!["Ticker", "Bid", "Ask", "Last", "Funding Rate"])
                .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
            let widths = vec![
                Constraint::Min(20),
                Constraint::Length(14),
                Constraint::Length(14),
                Constraint::Length(14),
                Constraint::Length(14),
            ];
            let rows: Vec<Row> = app.snapshots.iter().map(|s| {
                Row::new(vec![
                    Cell::from(s.ticker.clone()),
                    Cell::from(format_price(s.bid)),
                    Cell::from(format_price(s.ask)),
                    Cell::from(format_price(s.last)),
                    Cell::from(format_price(s.funding_rate)),
                ])
            }).collect();
            (header, widths, rows)
        }
        _ => {
            // polymarket or unknown
            let header = Row::new(vec!["Ticker", "Best Bid", "Best Ask", "Spread"])
                .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
            let widths = vec![
                Constraint::Min(20),
                Constraint::Length(12),
                Constraint::Length(12),
                Constraint::Length(12),
            ];
            let rows: Vec<Row> = app.snapshots.iter().map(|s| {
                Row::new(vec![
                    Cell::from(s.ticker.clone()),
                    Cell::from(format_price(s.best_bid)),
                    Cell::from(format_price(s.best_ask)),
                    Cell::from(format_price(s.spread)),
                ])
            }).collect();
            (header, widths, rows)
        }
    };

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" Market Data — {} ({}) ", feed, app.snapshots.len())),
        )
        .row_highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        );

    let mut state = TableState::default();
    if !app.snapshots.is_empty() {
        state.select(Some(app.snap_selected));
    }

    f.render_stateful_widget(table, area, &mut state);
}

fn draw_hints(f: &mut Frame, app: &App, area: Rect) {
    let hints = if app.show_confirmation {
        " y: confirm mass cancel | n/Esc: dismiss "
    } else {
        match app.active_tab {
            Tab::Orders => " q:quit  j/k:nav  n:new  c:cancel  p:pump  r:reconcile  x:mass-cancel  1-5:filter  Tab:switch ",
            Tab::Risk => " q:quit  Tab:switch  ?:help ",
            Tab::Positions => " q:quit  j/k:nav  Tab:switch  ?:help ",
            Tab::MarketData => " q:quit  j/k:nav  f:cycle-feed  Tab:switch  ?:help ",
            Tab::Help => " q:quit  Tab:switch  ?:help ",
        }
    };

    let hint_line = Paragraph::new(hints)
        .style(Style::default().bg(Color::DarkGray).fg(Color::White));
    f.render_widget(hint_line, area);
}

fn draw_order_form(f: &mut Frame, app: &App) {
    let form = match &app.order_form {
        Some(f) => f,
        None => return,
    };

    let area = f.area();
    let suggestion_lines = form.suggestions.len().min(5);
    let popup_width: u16 = 50;
    let popup_height: u16 = 13 + suggestion_lines as u16;
    let x = area.width.saturating_sub(popup_width) / 2;
    let y = area.height.saturating_sub(popup_height) / 2;
    let popup_area = Rect::new(x, y, popup_width.min(area.width), popup_height.min(area.height));

    f.render_widget(Clear, popup_area);

    let active = form.active_field;
    let highlight = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
    let normal = Style::default().fg(Color::White);
    let label_style = Style::default().fg(Color::Cyan);

    let field_line = |label: &str, value: &str, field: OrderField| -> Line {
        let style = if active == field { highlight } else { normal };
        let cursor = if active == field { "_" } else { "" };
        Line::from(vec![
            Span::styled(format!("  {:<10}", label), label_style),
            Span::styled(format!("{}{}", value, cursor), style),
        ])
    };

    let mut text = vec![
        Line::from(""),
        field_line("Feed:", &form.feed, OrderField::Feed),
        field_line("Ticker:", &form.ticker, OrderField::Ticker),
    ];

    // Show suggestions below ticker field
    if active == OrderField::Ticker && !form.suggestions.is_empty() {
        for (i, s) in form.suggestions.iter().take(5).enumerate() {
            let style = if i == form.suggestion_idx {
                Style::default().fg(Color::Black).bg(Color::Yellow)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            text.push(Line::from(Span::styled(format!("            {}", s), style)));
        }
    }

    text.push(field_line("Side:", &form.side, OrderField::Side));
    text.push(field_line("Action:", &form.action, OrderField::Action));
    text.push(field_line("Quantity:", &form.quantity, OrderField::Quantity));
    text.push(field_line("Price:", &form.price, OrderField::Price));
    text.push(Line::from(""));

    let hint = if active == OrderField::Feed {
        "  Enter/Space:cycle feed  Tab:next  Esc:cancel"
    } else if active == OrderField::Ticker && !form.suggestions.is_empty() {
        "  Up/Dn:browse  Tab/Enter:accept  Esc:cancel"
    } else {
        "  Tab:next  Enter:submit/toggle  Esc:cancel"
    };
    text.push(Line::from(Span::styled(hint, Style::default().fg(Color::DarkGray))));

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" New Order ")
        .border_style(Style::default().fg(Color::Cyan));

    let paragraph = Paragraph::new(text).block(block);
    f.render_widget(paragraph, popup_area);
}

fn draw_confirmation(f: &mut Frame) {
    let area = f.area();
    // Center a popup
    let popup_width = 44;
    let popup_height = 5;
    let x = area.width.saturating_sub(popup_width) / 2;
    let y = area.height.saturating_sub(popup_height) / 2;
    let popup_area = Rect::new(x, y, popup_width.min(area.width), popup_height.min(area.height));

    f.render_widget(Clear, popup_area);

    let text = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  Cancel ALL open orders?",
            Style::default()
                .fg(Color::Red)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from("  Press y to confirm, n/Esc to dismiss"),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Mass Cancel ")
        .border_style(Style::default().fg(Color::Red));

    let paragraph = Paragraph::new(text).block(block);
    f.render_widget(paragraph, popup_area);
}
