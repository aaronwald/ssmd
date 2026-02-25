pub mod app;
pub mod client;
pub mod event;
pub mod types;
pub mod ui;

use std::io;
use std::time::Duration;

use clap::Parser;
use crossterm::{
    event::Event,
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

/// Harman TUI - Terminal UI for the harman order management system
#[derive(Parser, Debug)]
#[command(name = "ssmd-harman-tui", about = "Terminal UI for harman order management")]
pub struct Args {
    /// Harman API base URL
    #[arg(long, default_value = "http://localhost:3000", env = "HARMAN_API_URL")]
    pub url: String,

    /// Bearer token for API authentication
    #[arg(long, env = "HARMAN_API_TOKEN")]
    pub token: String,

    /// Poll refresh interval in seconds
    #[arg(long, default_value_t = 2)]
    pub refresh: u64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let client = client::HarmanClient::new(args.url, args.token);
    let poll_interval = Duration::from_secs(args.refresh);

    let data_client = match (
        std::env::var("SSMD_API_URL"),
        std::env::var("SSMD_DATA_API_KEY"),
    ) {
        (Ok(url), Ok(key)) => {
            eprintln!("Market data enabled: {}", url);
            Some(client::DataClient::new(url, key))
        }
        _ => {
            eprintln!("Market data disabled (set SSMD_API_URL + SSMD_DATA_API_KEY to enable)");
            None
        }
    };

    let mut app = app::App::new(client, poll_interval, data_client);

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Install panic hook that restores terminal
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        original_hook(info);
    }));

    // Initial poll
    app.tick().await;

    let result = run_loop(&mut terminal, &mut app).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

async fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut app::App,
) -> anyhow::Result<()> {
    let event_timeout = Duration::from_millis(250);

    loop {
        // Render
        terminal.draw(|f| ui::draw(f, app))?;

        // Poll for events
        if let Some(Event::Key(key)) = event::poll_event(event_timeout)? {
            if !event::handle_key(app, key).await {
                break;
            }
        }

        // Check if tick interval has elapsed
        let should_tick = match app.last_poll {
            Some(t) => t.elapsed() >= app.poll_interval,
            None => true,
        };
        if should_tick {
            app.tick().await;
        }
    }

    Ok(())
}
