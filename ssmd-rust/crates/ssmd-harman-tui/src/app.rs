use std::time::{Duration, Instant};

use uuid::Uuid;

use crate::client::{DataClient, HarmanClient};
use crate::types::{CreateOrderRequest, Order, OrderState, RiskInfo, Snapshot};

/// Active tab in the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Orders,
    Risk,
    MarketData,
    Help,
}

const FEEDS: [&str; 3] = ["kalshi", "kraken-futures", "polymarket"];

/// Fields in the new order form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderField {
    Feed,
    Ticker,
    Side,
    Action,
    Quantity,
    Price,
}

const ORDER_FIELDS: [OrderField; 6] = [
    OrderField::Feed,
    OrderField::Ticker,
    OrderField::Side,
    OrderField::Action,
    OrderField::Quantity,
    OrderField::Price,
];

/// New order form state.
pub struct OrderForm {
    pub active_field: OrderField,
    pub feed: String,
    pub ticker: String,
    pub side: String,    // "yes" or "no"
    pub action: String,  // "buy" or "sell"
    pub quantity: String,
    pub price: String,
    pub suggestions: Vec<String>,
    pub suggestion_idx: usize,
    /// Tickers available for the selected feed (populated on feed change).
    pub feed_tickers: Vec<String>,
}

impl OrderForm {
    pub fn new(feed: &str) -> Self {
        Self {
            active_field: OrderField::Feed,
            feed: feed.to_string(),
            ticker: String::new(),
            side: "yes".to_string(),
            action: "buy".to_string(),
            quantity: "1".to_string(),
            price: String::new(),
            suggestions: Vec::new(),
            suggestion_idx: 0,
            feed_tickers: Vec::new(),
        }
    }

    pub fn cycle_feed(&mut self) {
        let current_idx = FEEDS.iter().position(|f| *f == self.feed).unwrap_or(0);
        let next_idx = (current_idx + 1) % FEEDS.len();
        self.feed = FEEDS[next_idx].to_string();
        // Clear ticker and suggestions when feed changes
        self.ticker.clear();
        self.suggestions.clear();
        self.suggestion_idx = 0;
        self.feed_tickers.clear();
    }

    pub fn next_field(&mut self) {
        let idx = ORDER_FIELDS.iter().position(|f| *f == self.active_field).unwrap_or(0);
        self.active_field = ORDER_FIELDS[(idx + 1) % ORDER_FIELDS.len()];
    }

    pub fn prev_field(&mut self) {
        let idx = ORDER_FIELDS.iter().position(|f| *f == self.active_field).unwrap_or(0);
        self.active_field = if idx == 0 { ORDER_FIELDS[ORDER_FIELDS.len() - 1] } else { ORDER_FIELDS[idx - 1] };
    }

    pub fn toggle_side(&mut self) {
        self.side = if self.side == "yes" { "no".to_string() } else { "yes".to_string() };
    }

    pub fn toggle_action(&mut self) {
        self.action = if self.action == "buy" { "sell".to_string() } else { "buy".to_string() };
    }

    pub fn active_input(&mut self) -> Option<&mut String> {
        match self.active_field {
            OrderField::Ticker => Some(&mut self.ticker),
            OrderField::Quantity => Some(&mut self.quantity),
            OrderField::Price => Some(&mut self.price),
            _ => None,
        }
    }

    /// Update suggestions from feed tickers based on current prefix.
    pub fn update_suggestions(&mut self) {
        if self.ticker.is_empty() {
            self.suggestions.clear();
            self.suggestion_idx = 0;
            return;
        }
        let prefix = self.ticker.to_uppercase();
        self.suggestions = self.feed_tickers
            .iter()
            .filter(|t| t.starts_with(&prefix))
            .take(8)
            .cloned()
            .collect();
        self.suggestion_idx = 0;
    }

    /// Accept the current suggestion into the ticker field.
    pub fn accept_suggestion(&mut self) {
        if let Some(s) = self.suggestions.get(self.suggestion_idx) {
            self.ticker = s.clone();
            self.suggestions.clear();
            self.suggestion_idx = 0;
        }
    }

    /// Cycle to next suggestion.
    pub fn next_suggestion(&mut self) {
        if !self.suggestions.is_empty() {
            self.suggestion_idx = (self.suggestion_idx + 1) % self.suggestions.len();
        }
    }

    /// Cycle to previous suggestion.
    pub fn prev_suggestion(&mut self) {
        if !self.suggestions.is_empty() {
            self.suggestion_idx = if self.suggestion_idx == 0 {
                self.suggestions.len() - 1
            } else {
                self.suggestion_idx - 1
            };
        }
    }

    pub fn to_request(&self) -> CreateOrderRequest {
        CreateOrderRequest {
            client_order_id: Uuid::new_v4(),
            ticker: self.ticker.clone(),
            side: self.side.clone(),
            action: self.action.clone(),
            quantity: self.quantity.clone(),
            price_dollars: self.price.clone(),
            time_in_force: "gtc".to_string(),
        }
    }
}

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
    // New order form
    pub order_form: Option<OrderForm>,
    // Market data
    pub data_client: Option<DataClient>,
    pub snapshots: Vec<Snapshot>,
    pub market_feed: String,
    pub snap_selected: usize,
    // Sorted ticker list for autocomplete (built from snapshots)
    pub known_tickers: Vec<String>,
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
            order_form: None,
            data_client,
            snapshots: Vec::new(),
            market_feed: "kalshi".to_string(),
            snap_selected: 0,
            known_tickers: Vec::new(),
        }
    }

    /// Poll orders and risk from the API. Called on each tick interval.
    pub async fn tick(&mut self) {
        match self.client.list_orders(None).await {
            Ok(mut orders) => {
                // Newest orders first
                orders.sort_by(|a, b| b.created_at.cmp(&a.created_at));
                self.orders = orders;
                self.last_error = None;
                // Collect unique tickers from orders into known_tickers
                let mut order_tickers: Vec<String> = self.orders.iter().map(|o| o.ticker.clone()).collect();
                order_tickers.sort();
                order_tickers.dedup();
                // Merge with existing known_tickers (from snapshots)
                for t in order_tickers {
                    if let Err(pos) = self.known_tickers.binary_search(&t) {
                        self.known_tickers.insert(pos, t);
                    }
                }
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
                    // Rebuild ticker list for autocomplete
                    let mut tickers: Vec<String> = snaps.iter().map(|s| s.ticker.clone()).collect();
                    tickers.sort();
                    tickers.dedup();
                    self.known_tickers = tickers;
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

    /// Return the order ID of the currently selected row (if any).
    pub fn selected_order_id(&self) -> Option<i64> {
        let filtered = self.filtered_orders();
        filtered.get(self.selected_index).map(|o| o.id)
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
