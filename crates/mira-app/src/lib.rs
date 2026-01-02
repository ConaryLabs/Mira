// crates/mira-app/src/lib.rs
// Mira Studio - Leptos WASM frontend (CSR)

use leptos::prelude::*;
use leptos_router::components::*;
use leptos_router::path;
use leptos_meta::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;
use web_sys::WebSocket;
use std::cell::RefCell;
use std::rc::Rc;

// Module declarations
pub mod ansi;
pub mod api;
pub mod components;
pub mod pages;
pub mod syntax;
pub mod websocket;

// Re-export shared types
pub use mira_types::*;

// Re-export for use in submodules
pub use components::{Layout, Nav, NotFound, ProjectSidebar};
pub use components::chat::{MessageBubble, TypingIndicator, ThinkingIndicator, Expandable, Markdown, CodeBlock};
pub use pages::*;

// ============================================================================
// Global Connection State (via Context)
// ============================================================================

#[derive(Clone, Copy)]
pub struct ConnectionState {
    pub connected: ReadSignal<bool>,
    pub set_connected: WriteSignal<bool>,
}

fn provide_connection_context() -> ConnectionState {
    let (connected, set_connected) = signal(false);
    let state = ConnectionState { connected, set_connected };
    provide_context(state);
    state
}

// ============================================================================
// WASM Entry Point
// ============================================================================

#[wasm_bindgen(start)]
pub fn main() {
    // Set up better panic messages
    console_error_panic_hook::set_once();

    // Initialize logging
    _ = console_log::init_with_level(log::Level::Debug);

    log::info!("Mira Studio starting...");

    // Mount the app
    leptos::mount::mount_to_body(App);
}

// ============================================================================
// App Root
// ============================================================================

#[component]
pub fn App() -> impl IntoView {
    provide_meta_context();
    let conn_state = provide_connection_context();

    // Global WebSocket connection
    let ws_ref: Rc<RefCell<Option<WebSocket>>> = Rc::new(RefCell::new(None));
    let ws_ref_clone = ws_ref.clone();

    Effect::new(move |_| {
        let ws_ref = ws_ref_clone.clone();
        spawn_local(async move {
            websocket::connect_websocket_global(ws_ref, conn_state.set_connected);
        });
    });

    view! {
        <Router>
            <Routes fallback=|| view! { <NotFound/> }>
                <Route path=path!("/") view=HomePage/>
                <Route path=path!("/ghost") view=GhostModePage/>
                <Route path=path!("/memories") view=MemoriesPage/>
                <Route path=path!("/code") view=CodePage/>
                <Route path=path!("/tasks") view=TasksPage/>
                <Route path=path!("/chat") view=ChatPage/>
            </Routes>
        </Router>
    }
}
