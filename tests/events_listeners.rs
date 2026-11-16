use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;

use headless_chrome::Browser;
use headless_chrome::browser::tab::{EventListener, Tab};
use headless_chrome::protocol::cdp::types::Event;

mod server;

#[test]
fn listen_to_events() -> Result<()> {
    let server = server::Server::with_dumb_html(include_str!("events_fixtures/events_page.html"));

    let counter_log_entries = Arc::new(Mutex::new(0));
    let counter_exception_thrown = Arc::new(Mutex::new(0));

    let browser = Browser::default()?;
    let tab: Arc<Tab> = browser.new_tab()?;

    tab.enable_log()?.enable_runtime()?;

    let counter_log_entries_clone = Arc::clone(&counter_log_entries);
    let counter_exception_thrown_clone = Arc::clone(&counter_exception_thrown);

    let sync_event = move |event: &Event| match event {
        Event::LogEntryAdded(_) => {
            *counter_log_entries_clone.lock().unwrap() += 1;
        }
        Event::RuntimeExceptionThrown(_) => {
            *counter_exception_thrown_clone.lock().unwrap() += 1;
        }
        _ => {}
    };

    tab.add_event_listener(Arc::new(sync_event))?;

    let url = format!("http://127.0.0.1:{}", server.port());
    tab.navigate_to(&url)?.wait_until_navigated()?;

    assert_eq!(*counter_log_entries.lock().unwrap(), 1);
    assert_eq!(*counter_exception_thrown.lock().unwrap(), 1);
    Ok(())
}

#[test]
fn remove_event_listener() -> Result<()> {
    let server = server::Server::with_dumb_html(include_str!("events_fixtures/events_page.html"));

    let counter_log_entries = Arc::new(Mutex::new(0));
    let counter_exception_thrown = Arc::new(Mutex::new(0));

    let browser = Browser::default()?;
    let tab: Arc<Tab> = browser.new_tab()?;

    tab.enable_log()?.enable_runtime()?;

    let counter_log_entries_clone = Arc::clone(&counter_log_entries);
    let counter_exception_thrown_clone = Arc::clone(&counter_exception_thrown);

    let sync_event = move |event: &Event| {
        if let Event::LogEntryAdded(_) = event {
            *counter_log_entries_clone.lock().unwrap() += 1;
        }
    };

    let runtime_event = move |event: &Event| {
        if let Event::RuntimeExceptionThrown(_) = event {
            *counter_exception_thrown_clone.lock().unwrap() += 1;
        }
    };

    tab.add_event_listener(Arc::new(sync_event))?;

    let runtime_listener = tab.add_event_listener(Arc::new(runtime_event))?;

    tab.remove_event_listener(&runtime_listener)?;

    let url = format!("http://127.0.0.1:{}", server.port());
    tab.navigate_to(&url)?.wait_until_navigated()?;

    assert_eq!(*counter_log_entries.lock().unwrap(), 1);
    assert_eq!(*counter_exception_thrown.lock().unwrap(), 0);
    Ok(())
}

/// Dropping a tab has to end its event handling thread, otherwise every tab ever
/// opened keeps its state (and its thread) alive until the browser goes away.
#[test]
fn tab_state_is_released_when_tab_is_dropped() -> Result<()> {
    let browser = Browser::default()?;
    let tab: Arc<Tab> = browser.new_tab()?;

    let listener: Arc<dyn EventListener<Event> + Send + Sync> = Arc::new(|_event: &Event| {});
    // Held by the tab, which the tab's event handling thread holds in turn.
    let weak_listener = tab.add_event_listener(Arc::clone(&listener))?;
    drop(listener);
    assert!(weak_listener.upgrade().is_some());

    tab.close(false)?;
    drop(tab);

    let deadline = Instant::now() + Duration::from_secs(10);
    while weak_listener.upgrade().is_some() {
        assert!(
            Instant::now() < deadline,
            "tab's event handling thread is still holding on to the tab's state"
        );
        std::thread::sleep(Duration::from_millis(50));
    }

    Ok(())
}
