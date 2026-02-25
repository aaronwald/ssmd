use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use std::time::Duration;

use crate::app::{App, Tab};
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

        KeyCode::Char('j') | KeyCode::Down => {
            if app.active_tab == Tab::MarketData {
                app.snap_next();
            } else {
                app.select_next();
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if app.active_tab == Tab::MarketData {
                app.snap_prev();
            } else {
                app.select_prev();
            }
        }

        KeyCode::Tab => app.next_tab(),
        KeyCode::Char('?') => {
            app.active_tab = if app.active_tab == Tab::Help {
                Tab::Orders
            } else {
                Tab::Help
            };
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
