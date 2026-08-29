mod app;
mod feed;
mod keys;
mod reader;
mod store;
mod ui;

use std::path::PathBuf;
use std::time::Duration;

use crossterm::event::{Event, EventStream};
use directories::{BaseDirs, ProjectDirs};
use futures_util::StreamExt;
use tokio::sync::mpsc;

use app::{App, ReaderEvent};
use feed::fetch::FetchEvent;

fn resolve_paths() -> anyhow::Result<(PathBuf, PathBuf)> {
    // Prefer an OPML file next to the binary/cwd (matches the old repo-local
    // convention), fall back to the XDG config dir.
    let local_opml = PathBuf::from("feeds.opml");
    let opml_path = if local_opml.exists() {
        local_opml
    } else {
        let base = BaseDirs::new().ok_or_else(|| anyhow::anyhow!("no home directory"))?;
        base.config_dir().join("dnews").join("feeds.opml")
    };

    let proj =
        ProjectDirs::from("", "", "dnews").ok_or_else(|| anyhow::anyhow!("no home directory"))?;
    let data_dir = proj.data_dir().to_path_buf();
    std::fs::create_dir_all(&data_dir)?;
    let db_path = data_dir.join("dnews.db");

    Ok((opml_path, db_path))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (opml_path, db_path) = resolve_paths()?;

    if !opml_path.exists() {
        eprintln!(
            "No feeds.opml found at {} (or ./feeds.opml). Create one first.",
            opml_path.display()
        );
        std::process::exit(1);
    }

    let feeds = feed::opml::parse(&opml_path)?;
    if feeds.is_empty() {
        eprintln!("feeds.opml has no feeds with an xmlUrl.");
        std::process::exit(1);
    }

    let db = store::Store::open(&db_path)?;
    db.init_schema()?;

    let mut app = App::new(db, feeds);
    app.prune_old_articles();

    // Deliberately not enabling mouse capture: doing so would take over the
    // terminal's native click-drag text selection (copy/paste), which
    // matters more here than custom scroll-wheel handling — most terminals
    // still translate the wheel into Up/Down key presses on their own when
    // capture is off, so scrolling still works, just via `keys::handle`.
    let mut terminal = ratatui::init();
    let result = run(&mut terminal, &mut app).await;
    ratatui::restore();
    result
}

async fn run(terminal: &mut ratatui::DefaultTerminal, app: &mut App) -> anyhow::Result<()> {
    let (progress_tx, mut progress_rx) = mpsc::unbounded_channel::<FetchEvent>();
    let (reader_tx, mut reader_rx) = mpsc::unbounded_channel::<ReaderEvent>();
    app.start_reload(&progress_tx);

    app.reload_view()?;
    terminal.draw(|f| ui::draw(f, app))?;

    let mut events = EventStream::new();

    loop {
        if app.should_quit {
            break;
        }
        let wide = terminal.size().map(|s| s.width).unwrap_or(0) >= ui::SPLIT_MIN_WIDTH;

        tokio::select! {
            maybe_event = events.next() => {
                match maybe_event {
                    Some(Ok(Event::Key(key))) => {
                        keys::handle(app, key, &progress_tx, &reader_tx, wide)?;
                    }
                    Some(Ok(Event::Resize(_, _))) => {}
                    Some(Ok(_)) => continue,
                    Some(Err(e)) => return Err(e.into()),
                    None => break,
                }
            }
            progress = progress_rx.recv() => {
                match progress {
                    Some(FetchEvent::Started(total)) => {
                        app.loading = true;
                        app.total_feeds = total;
                        app.done_feeds = 0;
                    }
                    Some(FetchEvent::FeedDone) => {
                        app.done_feeds += 1;
                    }
                    Some(FetchEvent::Complete) => {
                        app.loading = false;
                        app.prune_old_articles();
                        app.reload_view()?;
                    }
                    Some(FetchEvent::Error(msg)) => {
                        app.status = format!("⚠ {msg}");
                    }
                    None => {}
                }
            }
            reader_event = reader_rx.recv() => {
                if let Some(event) = reader_event {
                    app.apply_reader_event(event);
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(90)), if app.loading || app.reader_loading => {
                app.spin_frame = app.spin_frame.wrapping_add(1);
            }
            _ = async {
                match app.preview_due_at {
                    Some(due) => tokio::time::sleep_until(due.into()).await,
                    None => std::future::pending::<()>().await,
                }
            } => {
                app.preview_due_at = None;
                if wide {
                    app.activate_selected(&reader_tx);
                }
            }
        }

        terminal.draw(|f| ui::draw(f, app))?;
    }

    Ok(())
}
