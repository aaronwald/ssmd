use std::time::{Duration, Instant};

use crate::client::{DataClient, HarmanClient};
use crate::types::{Order, OrderState, RiskInfo, Snapshot};

/// Active tab in the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Orders,
    Risk,
    MarketData,
    Help,
}

const FEEDS: [&str; 3] = ["kalshi", "kraken-futures", "polymarket"];

/// All filter options, including "All" (no filter).
const FILTERS: [Option<OrderState>; 5] = [
    None,
    Some(OrderState::Pending),
    Some(OrderState::Acknowledged),
    Some(OrderState::Filled),
    Some(OrderState::Cancelled),
];

pub struct App {
    pub client: HarmanClient,
    pub orders: Vec<Order>,
    pub risk: Option<RiskInfo>,
    pub selected_index: usize,
    pub active_tab: Tab,
    pub state_filter: Option<OrderState>,
    pub last_poll: Option<Instant>,
    pub last_error: Option<String>,
    pub show_confirmation: bool,
    pub poll_interval: Duration,
    pub last_action_result: Option<String>,
    pub running: bool,
    // Market data
    pub data_client: Option<DataClient>,
    pub snapshots: Vec<Snapshot>,
    pub market_feed: String,
    pub snap_selected: usize,
}

impl App {
    pub fn new(client: HarmanClient, poll_interval: Duration, data_client: Option<DataClient>) -> Self {
        Self {
            client,
            orders: Vec::new(),
            risk: None,
            selected_index: 0,
            active_tab: Tab::Orders,
            state_filter: None,
            last_poll: None,
            last_error: None,
            show_confirmation: false,
            poll_interval,
            last_action_result: None,
            running: true,
            data_client,
            snapshots: Vec::new(),
            market_feed: "kalshi".to_string(),
            snap_selected: 0,
        }
    }

    /// Poll orders and risk from the API. Called on each tick interval.
    pub async fn tick(&mut self) {
        match self.client.list_orders(None).await {
            Ok(orders) => {
                self.orders = orders;
                self.last_error = None;
            }
            Err(e) => {
                self.last_error = Some(format!("orders: {}", e));
            }
        }

        match self.client.get_risk().await {
            Ok(risk) => {
                self.risk = Some(risk);
                // Only clear error if orders also succeeded
                if self.last_error.is_none() {
                    self.last_error = None;
                }
            }
            Err(e) => {
                let msg = format!("risk: {}", e);
                self.last_error = Some(match &self.last_error {
                    Some(existing) => format!("{}; {}", existing, msg),
                    None => msg,
                });
            }
        }

        // Poll market data snapshots if data_client is configured
        if let Some(ref dc) = self.data_client {
            match dc.snap(&self.market_feed, None).await {
                Ok(snaps) => {
                    self.snapshots = snaps;
                }
                Err(e) => {
                    let msg = format!("snap: {}", e);
                    self.last_error = Some(match &self.last_error {
                        Some(existing) => format!("{}; {}", existing, msg),
                        None => msg,
                    });
                }
            }
        }

        self.last_poll = Some(Instant::now());

        // Clamp selected index to filtered list bounds
        let len = self.filtered_orders().len();
        if len == 0 {
            self.selected_index = 0;
        } else if self.selected_index >= len {
            self.selected_index = len - 1;
        }

        // Clamp snap_selected
        if self.snapshots.is_empty() {
            self.snap_selected = 0;
        } else if self.snap_selected >= self.snapshots.len() {
            self.snap_selected = self.snapshots.len() - 1;
        }
    }

    /// Return orders filtered by the current state_filter.
    pub fn filtered_orders(&self) -> Vec<&Order> {
        match self.state_filter {
            None => self.orders.iter().collect(),
            Some(state) => self.orders.iter().filter(|o| o.state == state).collect(),
        }
    }

    /// Cycle to the next state filter.
    pub fn next_filter(&mut self) {
        let current_idx = FILTERS.iter().position(|f| *f == self.state_filter).unwrap_or(0);
        let next_idx = (current_idx + 1) % FILTERS.len();
        self.state_filter = FILTERS[next_idx];
        self.clamp_selected();
    }

    /// Cycle to the previous state filter.
    pub fn prev_filter(&mut self) {
        let current_idx = FILTERS.iter().position(|f| *f == self.state_filter).unwrap_or(0);
        let next_idx = if current_idx == 0 {
            FILTERS.len() - 1
        } else {
            current_idx - 1
        };
        self.state_filter = FILTERS[next_idx];
        self.clamp_selected();
    }

    /// Select the next order in the filtered list.
    pub fn select_next(&mut self) {
        let len = self.filtered_orders().len();
        if len > 0 && self.selected_index < len - 1 {
            self.selected_index += 1;
        }
    }

    /// Select the previous order in the filtered list.
    pub fn select_prev(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    /// Cycle to the next tab.
    pub fn next_tab(&mut self) {
        self.active_tab = match self.active_tab {
            Tab::Orders => Tab::Risk,
            Tab::Risk => {
                if self.data_client.is_some() {
                    Tab::MarketData
                } else {
                    Tab::Help
                }
            }
            Tab::MarketData => Tab::Help,
            Tab::Help => Tab::Orders,
        };
    }

    /// Cycle feed in market data tab.
    pub fn cycle_feed(&mut self) {
        let current_idx = FEEDS.iter().position(|f| *f == self.market_feed).unwrap_or(0);
        let next_idx = (current_idx + 1) % FEEDS.len();
        self.market_feed = FEEDS[next_idx].to_string();
        self.snap_selected = 0;
    }

    /// Select the next snapshot row.
    pub fn snap_next(&mut self) {
        if !self.snapshots.is_empty() && self.snap_selected < self.snapshots.len() - 1 {
            self.snap_selected += 1;
        }
    }

    /// Select the previous snapshot row.
    pub fn snap_prev(&mut self) {
        if self.snap_selected > 0 {
            self.snap_selected -= 1;
        }
    }

    /// Whether the market data tab is available.
    pub fn has_market_data(&self) -> bool {
        self.data_client.is_some()
    }

    fn clamp_selected(&mut self) {
        let len = self.filtered_orders().len();
        if len == 0 {
            self.selected_index = 0;
        } else if self.selected_index >= len {
            self.selected_index = len - 1;
        }
    }
}
