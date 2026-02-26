use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use std::time::Duration;

use crate::app::{App, OrderField, OrderForm, Tab};
use crate::types::OrderState;

/// Poll for a crossterm event with the given timeout.
/// Returns None if no event arrived within the timeout.
pub fn poll_event(timeout: Duration) -> std::io::Result<Option<Event>> {
    if event::poll(timeout)? {
        Ok(Some(event::read()?))
    } else {
        Ok(None)
    }
}

/// Handle a key event. Returns false if the app should quit.
pub async fn handle_key(app: &mut App, key: KeyEvent) -> bool {
    // Ctrl+C always quits
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return false;
    }

    // Order form mode
    if app.order_form.is_some() {
        handle_order_form(app, key).await;
        return true;
    }

    // Confirmation dialog mode
    if app.show_confirmation {
        match key.code {
            KeyCode::Char('y') => {
                app.show_confirmation = false;
                match app.client.mass_cancel().await {
                    Ok(result) => {
                        app.last_action_result =
                            Some(format!("mass cancel: {} cancelled", result.cancelled));
                    }
                    Err(e) => {
                        app.last_action_result = Some(format!("mass cancel failed: {}", e));
                    }
                }
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                app.show_confirmation = false;
            }
            _ => {}
        }
        return true;
    }

    // Normal mode
    match key.code {
        KeyCode::Char('q') => return false,

        KeyCode::Char('j') | KeyCode::Down => match app.active_tab {
            Tab::MarketData => app.snap_next(),
            Tab::Positions => app.pos_next(),
            _ => app.select_next(),
        },
        KeyCode::Char('k') | KeyCode::Up => match app.active_tab {
            Tab::MarketData => app.snap_prev(),
            Tab::Positions => app.pos_prev(),
            _ => app.select_prev(),
        },

        KeyCode::Tab => app.next_tab(),
        KeyCode::Char('?') => {
            app.active_tab = if app.active_tab == Tab::Help {
                Tab::Orders
            } else {
                Tab::Help
            };
        }

        KeyCode::Char('n') => {
            if app.active_tab == Tab::Orders {
                let mut form = OrderForm::new(&app.market_feed);
                // Pre-populate feed tickers from current known_tickers
                form.feed_tickers = app.known_tickers.clone();
                app.order_form = Some(form);
            }
        }

        KeyCode::Char('p') => {
            match app.client.pump().await {
                Ok(result) => {
                    app.last_action_result = Some(format!(
                        "pump: {} processed, {} submitted, {} rejected",
                        result.processed, result.submitted, result.rejected
                    ));
                }
                Err(e) => {
                    app.last_action_result = Some(format!("pump failed: {}", e));
                }
            }
        }

        KeyCode::Char('r') => {
            match app.client.reconcile().await {
                Ok(result) => {
                    app.last_action_result = Some(format!(
                        "reconcile: {} fills, {} resolved",
                        result.fills_discovered, result.orders_resolved
                    ));
                }
                Err(e) => {
                    app.last_action_result = Some(format!("reconcile failed: {}", e));
                }
            }
        }

        KeyCode::Char('c') => {
            if app.active_tab == Tab::Orders {
                if let Some(id) = app.selected_order_id() {
                    match app.client.cancel_order(id).await {
                        Ok(()) => {
                            app.last_action_result = Some(format!("cancel order #{}", id));
                        }
                        Err(e) => {
                            app.last_action_result = Some(format!("cancel #{} failed: {}", id, e));
                        }
                    }
                }
            }
        }

        KeyCode::Char('x') => {
            app.show_confirmation = true;
        }

        KeyCode::Char('f') => {
            if app.active_tab == Tab::MarketData {
                app.cycle_feed();
            }
        }

        // Number filters
        KeyCode::Char('1') => {
            app.state_filter = None;
            app.selected_index = 0;
        }
        KeyCode::Char('2') => {
            app.state_filter = Some(OrderState::Pending);
            app.selected_index = 0;
        }
        KeyCode::Char('3') => {
            app.state_filter = Some(OrderState::Acknowledged);
            app.selected_index = 0;
        }
        KeyCode::Char('4') => {
            app.state_filter = Some(OrderState::Filled);
            app.selected_index = 0;
        }
        KeyCode::Char('5') => {
            app.state_filter = Some(OrderState::Cancelled);
            app.selected_index = 0;
        }

        _ => {}
    }

    true
}

async fn handle_order_form(app: &mut App, key: KeyEvent) {
    let form = match app.order_form.as_mut() {
        Some(f) => f,
        None => return,
    };

    match key.code {
        KeyCode::Esc => {
            app.order_form = None;
        }
        KeyCode::Tab => {
            // In Ticker field with suggestions: accept suggestion
            if form.active_field == OrderField::Ticker && !form.suggestions.is_empty() {
                form.accept_suggestion();
            } else {
                form.next_field();
            }
        }
        KeyCode::Down => {
            // In Ticker field with suggestions: cycle suggestions
            if form.active_field == OrderField::Ticker && !form.suggestions.is_empty() {
                form.next_suggestion();
            } else {
                form.next_field();
            }
        }
        KeyCode::Up => {
            if form.active_field == OrderField::Ticker && !form.suggestions.is_empty() {
                form.prev_suggestion();
            } else {
                form.prev_field();
            }
        }
        KeyCode::BackTab => {
            form.prev_field();
        }
        KeyCode::Enter => {
            match form.active_field {
                OrderField::Feed => {
                    form.cycle_feed();
                    // Fetch tickers for the new feed
                    if let Some(ref dc) = app.data_client {
                        if let Ok(snaps) = dc.snap(&form.feed, None).await {
                            let mut tickers: Vec<String> = snaps.iter().map(|s| s.ticker.clone()).collect();
                            tickers.sort();
                            tickers.dedup();
                            if let Some(f) = app.order_form.as_mut() {
                                f.feed_tickers = tickers;
                            }
                        }
                    }
                }
                OrderField::Side => form.toggle_side(),
                OrderField::Action => form.toggle_action(),
                OrderField::Ticker if !form.suggestions.is_empty() => {
                    form.accept_suggestion();
                }
                _ => {
                    // Submit the order
                    let req = form.to_request();
                    app.order_form = None;
                    match app.client.create_order(&req).await {
                        Ok(resp) => {
                            let id = resp.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
                            app.last_action_result = Some(format!("created order #{}", id));
                        }
                        Err(e) => {
                            app.last_action_result = Some(format!("create failed: {}", e));
                        }
                    }
                }
            }
        }
        KeyCode::Char(' ') => {
            match form.active_field {
                OrderField::Feed => {
                    form.cycle_feed();
                    if let Some(ref dc) = app.data_client {
                        if let Ok(snaps) = dc.snap(&form.feed, None).await {
                            let mut tickers: Vec<String> = snaps.iter().map(|s| s.ticker.clone()).collect();
                            tickers.sort();
                            tickers.dedup();
                            if let Some(f) = app.order_form.as_mut() {
                                f.feed_tickers = tickers;
                            }
                        }
                    }
                }
                OrderField::Side => form.toggle_side(),
                OrderField::Action => form.toggle_action(),
                _ => {
                    if let Some(input) = form.active_input() {
                        input.push(' ');
                    }
                }
            }
        }
        KeyCode::Backspace => {
            if let Some(input) = form.active_input() {
                input.pop();
            }
            if form.active_field == OrderField::Ticker {
                form.update_suggestions();
            }
        }
        KeyCode::Char(c) => {
            if let Some(input) = form.active_input() {
                input.push(c);
            }
            if form.active_field == OrderField::Ticker {
                form.update_suggestions();
            }
        }
        _ => {}
    }
}
