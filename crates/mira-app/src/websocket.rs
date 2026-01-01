// crates/mira-app/src/websocket.rs
// WebSocket connection and event handling

use std::cell::RefCell;
use std::rc::Rc;
use leptos::prelude::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;
use web_sys::{WebSocket, MessageEvent, CloseEvent, ErrorEvent};
use mira_types::{WsEvent, WsCommand};

/// Extract event ID from call_id (format: "replay-{id}" for replayed events)
fn parse_event_id(call_id: &str) -> Option<i64> {
    call_id.strip_prefix("replay-").and_then(|id| id.parse().ok())
}

pub fn connect_websocket(
    ws_ref: Rc<RefCell<Option<WebSocket>>>,
    set_events: WriteSignal<Vec<WsEvent>>,
    set_connected: WriteSignal<bool>,
) {
    // Track last event ID for sync on reconnect
    let last_event_id: Rc<RefCell<Option<i64>>> = Rc::new(RefCell::new(None));

    connect_websocket_with_sync(
        ws_ref,
        set_events,
        set_connected,
        last_event_id,
        0, // Initial reconnect attempt
    );
}

fn connect_websocket_with_sync(
    ws_ref: Rc<RefCell<Option<WebSocket>>>,
    set_events: WriteSignal<Vec<WsEvent>>,
    set_connected: WriteSignal<bool>,
    last_event_id: Rc<RefCell<Option<i64>>>,
    reconnect_attempt: u32,
) {
    let window = web_sys::window().expect("No window");
    let location = window.location();
    let host = location.host().expect("No host");
    let protocol = location.protocol().expect("No protocol");

    let ws_protocol = if protocol == "https:" { "wss:" } else { "ws:" };
    let ws_url = format!("{}//{}/ws", ws_protocol, host);

    log::info!("Connecting to WebSocket: {} (attempt {})", ws_url, reconnect_attempt + 1);

    let ws = match WebSocket::new(&ws_url) {
        Ok(ws) => ws,
        Err(e) => {
            log::error!("Failed to create WebSocket: {:?}", e);
            return;
        }
    };

    ws.set_binary_type(web_sys::BinaryType::Arraybuffer);

    // On open - send sync command if we have a last event ID
    let set_connected_clone = set_connected;
    let last_event_id_clone = last_event_id.clone();
    let ws_clone = ws.clone();
    let onopen = Closure::wrap(Box::new(move |_: web_sys::Event| {
        log::info!("WebSocket connected");
        set_connected_clone.set(true);

        // If we have a last event ID, send sync command
        if let Some(id) = *last_event_id_clone.borrow() {
            let sync_cmd = WsCommand::Sync { last_event_id: Some(id) };
            if let Ok(json) = serde_json::to_string(&sync_cmd) {
                log::info!("Sending sync from event {}", id);
                let _ = ws_clone.send_with_str(&json);
            }
        }
    }) as Box<dyn FnMut(_)>);
    ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
    onopen.forget();

    // On message - track event IDs from ToolResult events
    let set_events_clone = set_events;
    let last_event_id_clone = last_event_id.clone();
    let onmessage = Closure::wrap(Box::new(move |e: MessageEvent| {
        if let Ok(text) = e.data().dyn_into::<js_sys::JsString>() {
            let text: String = text.into();
            match serde_json::from_str::<WsEvent>(&text) {
                Ok(event) => {
                    log::debug!("Received event: {:?}", event);

                    // Track the highest event ID from ToolResult events
                    if let WsEvent::ToolResult { ref call_id, .. } = event {
                        if let Some(id) = parse_event_id(call_id) {
                            let mut last_id = last_event_id_clone.borrow_mut();
                            if last_id.map_or(true, |prev| id > prev) {
                                *last_id = Some(id);
                            }
                        }
                    }

                    set_events_clone.update(|events| events.push(event));
                }
                Err(e) => {
                    log::warn!("Failed to parse WsEvent: {:?}", e);
                }
            }
        }
    }) as Box<dyn FnMut(_)>);
    ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
    onmessage.forget();

    // On close - reconnect with exponential backoff
    let set_connected_clone2 = set_connected;
    let ws_ref_clone = ws_ref.clone();
    let last_event_id_clone = last_event_id.clone();
    let onclose = Closure::wrap(Box::new(move |e: CloseEvent| {
        log::info!("WebSocket closed: code={}, reason={}", e.code(), e.reason());
        set_connected_clone2.set(false);

        // Reconnect with exponential backoff (max 30 seconds)
        let delay_ms = std::cmp::min(1000 * 2u32.pow(reconnect_attempt), 30000);
        log::info!("Reconnecting in {}ms...", delay_ms);

        let ws_ref = ws_ref_clone.clone();
        let last_event_id = last_event_id_clone.clone();
        let next_attempt = reconnect_attempt + 1;

        spawn_local(async move {
            gloo_timers::future::TimeoutFuture::new(delay_ms).await;
            connect_websocket_with_sync(
                ws_ref,
                set_events,
                set_connected,
                last_event_id,
                next_attempt,
            );
        });
    }) as Box<dyn FnMut(_)>);
    ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));
    onclose.forget();

    // On error
    let onerror = Closure::wrap(Box::new(move |e: ErrorEvent| {
        log::error!("WebSocket error: {:?}", e.message());
    }) as Box<dyn FnMut(_)>);
    ws.set_onerror(Some(onerror.as_ref().unchecked_ref()));
    onerror.forget();

    // Store the WebSocket
    *ws_ref.borrow_mut() = Some(ws);
}

// Global WebSocket connection (just for connection status, no event handling)
pub fn connect_websocket_global(
    ws_ref: Rc<RefCell<Option<WebSocket>>>,
    set_connected: WriteSignal<bool>,
) {
    let window = web_sys::window().expect("No window");
    let location = window.location();
    let host = location.host().expect("No host");
    let protocol = location.protocol().expect("No protocol");

    let ws_protocol = if protocol == "https:" { "wss:" } else { "ws:" };
    let ws_url = format!("{}//{}/ws", ws_protocol, host);

    log::info!("Connecting to WebSocket: {}", ws_url);

    let ws = match WebSocket::new(&ws_url) {
        Ok(ws) => ws,
        Err(e) => {
            log::error!("Failed to create WebSocket: {:?}", e);
            return;
        }
    };

    ws.set_binary_type(web_sys::BinaryType::Arraybuffer);

    // On open
    let set_connected_clone = set_connected;
    let onopen = Closure::wrap(Box::new(move |_: web_sys::Event| {
        log::info!("WebSocket connected");
        set_connected_clone.set(true);
    }) as Box<dyn FnMut(_)>);
    ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
    onopen.forget();

    // On close
    let set_connected_clone2 = set_connected;
    let onclose = Closure::wrap(Box::new(move |e: CloseEvent| {
        log::info!("WebSocket closed: code={}, reason={}", e.code(), e.reason());
        set_connected_clone2.set(false);
    }) as Box<dyn FnMut(_)>);
    ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));
    onclose.forget();

    // On error
    let onerror = Closure::wrap(Box::new(move |e: ErrorEvent| {
        log::error!("WebSocket error: {:?}", e.message());
    }) as Box<dyn FnMut(_)>);
    ws.set_onerror(Some(onerror.as_ref().unchecked_ref()));
    onerror.forget();

    // Store the WebSocket
    *ws_ref.borrow_mut() = Some(ws);
}
