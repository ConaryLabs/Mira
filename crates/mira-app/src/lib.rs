// mira-app: Leptos WASM frontend for Mira Studio
// Client-side rendered (CSR) application

use leptos::prelude::*;
use leptos_router::components::*;
use leptos_router::path;
use leptos_meta::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;
use web_sys::{WebSocket, MessageEvent, CloseEvent, ErrorEvent};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::OnceLock;
use syntect::parsing::SyntaxSet;
use syntect::highlighting::ThemeSet;
use syntect::html::{styled_line_to_highlighted_html, IncludeBackground};
use syntect::easy::HighlightLines;

// Re-export shared types
pub use mira_types::*;

// ============================================================================
// Syntax Highlighting (syntect - pure Rust)
// ============================================================================

static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
static THEME_SET: OnceLock<ThemeSet> = OnceLock::new();

fn get_syntax_set() -> &'static SyntaxSet {
    SYNTAX_SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn get_theme_set() -> &'static ThemeSet {
    THEME_SET.get_or_init(ThemeSet::load_defaults)
}

/// Highlight a line of code, returning HTML with inline styles
fn highlight_line(code: &str, extension: &str) -> String {
    let ss = get_syntax_set();
    let ts = get_theme_set();

    let syntax = ss.find_syntax_by_extension(extension)
        .unwrap_or_else(|| ss.find_syntax_plain_text());

    // Use a dark theme that matches our UI
    let theme = ts.themes.get("base16-ocean.dark")
        .or_else(|| ts.themes.get("InspiredGitHub"))
        .unwrap_or_else(|| ts.themes.values().next().unwrap());

    let mut highlighter = HighlightLines::new(syntax, theme);

    match highlighter.highlight_line(code, ss) {
        Ok(ranges) => styled_line_to_highlighted_html(&ranges[..], IncludeBackground::No)
            .unwrap_or_else(|_| html_escape(code)),
        Err(_) => html_escape(code),
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Get file extension from path
fn get_extension(path: &str) -> &str {
    path.rsplit('.').next().unwrap_or("")
}

// ============================================================================
// Global Connection State (via Context)
// ============================================================================

#[derive(Clone, Copy)]
struct ConnectionState {
    connected: ReadSignal<bool>,
    set_connected: WriteSignal<bool>,
}

fn provide_connection_context() -> ConnectionState {
    let (connected, set_connected) = signal(false);
    let state = ConnectionState { connected, set_connected };
    provide_context(state);
    state
}

fn use_connection_state() -> ConnectionState {
    expect_context::<ConnectionState>()
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
            connect_websocket_global(ws_ref, conn_state.set_connected);
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
            </Routes>
        </Router>
    }
}

// ============================================================================
// Layout Components
// ============================================================================

#[component]
fn Layout(children: Children) -> impl IntoView {
    view! {
        <div class="min-h-screen flex flex-col">
            <Nav/>
            <main class="flex-1 p-4">
                {children()}
            </main>
        </div>
    }
}

#[component]
fn Nav() -> impl IntoView {
    view! {
        <nav class="border-b border-border px-4 py-3 flex items-center gap-6">
            <a href="/" class="text-accent font-bold text-lg">"Mira Studio"</a>
            <div class="flex gap-4 text-sm">
                <a href="/ghost" class="hover:text-accent transition-colors">"Ghost Mode"</a>
                <a href="/memories" class="hover:text-accent transition-colors">"Memories"</a>
                <a href="/code" class="hover:text-accent transition-colors">"Code"</a>
                <a href="/tasks" class="hover:text-accent transition-colors">"Tasks"</a>
            </div>
            <div class="flex-1"></div>
            <ConnectionStatus/>
        </nav>
    }
}

#[component]
fn ConnectionStatus() -> impl IntoView {
    let conn_state = use_connection_state();

    view! {
        <div class="flex items-center gap-2 text-xs">
            <div class=move || {
                if conn_state.connected.get() {
                    "w-2 h-2 rounded-full bg-success"
                } else {
                    "w-2 h-2 rounded-full bg-error"
                }
            }></div>
            <span class="text-muted">
                {move || if conn_state.connected.get() { "Connected" } else { "Disconnected" }}
            </span>
        </div>
    }
}

#[component]
fn NotFound() -> impl IntoView {
    view! {
        <Layout>
            <div class="max-w-4xl mx-auto py-12 text-center">
                <h1 class="text-4xl font-bold text-error mb-4">"404"</h1>
                <p class="text-muted mb-8">"Page not found"</p>
                <a href="/" class="text-accent hover:underline">"Go home"</a>
            </div>
        </Layout>
    }
}

// ============================================================================
// Home Page
// ============================================================================

#[component]
fn HomePage() -> impl IntoView {
    let (status, set_status) = signal("Checking...".to_string());

    // Check server health on mount
    Effect::new(move |_| {
        spawn_local(async move {
            match fetch_health().await {
                Ok(health) => set_status.set(format!("Connected - {}", health)),
                Err(e) => set_status.set(format!("Error: {}", e)),
            }
        });
    });

    view! {
        <Layout>
            <div class="max-w-4xl mx-auto py-12 text-center">
                <h1 class="text-4xl font-bold text-accent mb-4">"Mira Studio"</h1>
                <p class="text-muted mb-8">"Memory and Intelligence Layer for Claude Code"</p>

                <div class="grid grid-cols-2 gap-4 max-w-2xl mx-auto">
                    <HomeCard
                        title="Ghost Mode"
                        href="/ghost"
                        description="Real-time agent reasoning visualization"
                    />
                    <HomeCard
                        title="Memories"
                        href="/memories"
                        description="Semantic memory storage and search"
                    />
                    <HomeCard
                        title="Code Intel"
                        href="/code"
                        description="Code symbols and semantic search"
                    />
                    <HomeCard
                        title="Tasks"
                        href="/tasks"
                        description="Goals and task management"
                    />
                </div>

                <div class="mt-12 p-4 bg-card rounded-lg border border-border">
                    <h3 class="text-sm text-muted mb-2">"Server Status"</h3>
                    <p class=move || {
                        if status.get().starts_with("Connected") {
                            "text-success"
                        } else if status.get().starts_with("Error") {
                            "text-error"
                        } else {
                            "text-muted"
                        }
                    }>
                        {move || status.get()}
                    </p>
                </div>
            </div>
        </Layout>
    }
}

#[component]
fn HomeCard(
    title: &'static str,
    href: &'static str,
    description: &'static str,
) -> impl IntoView {
    view! {
        <a
            href=href
            class="block p-6 bg-card rounded-lg border border-border hover:border-accent transition-colors"
        >
            <h3 class="text-lg font-semibold mb-2">{title}</h3>
            <p class="text-sm text-muted">{description}</p>
        </a>
    }
}

// ============================================================================
// Ghost Mode Page
// ============================================================================

#[component]
fn GhostModePage() -> impl IntoView {
    // WebSocket connection and event state
    let (events, set_events) = signal(Vec::<WsEvent>::new());
    let (ws_connected, set_ws_connected) = signal(false);
    let ws_ref: Rc<RefCell<Option<WebSocket>>> = Rc::new(RefCell::new(None));

    // Connect to WebSocket on mount
    let ws_ref_clone = ws_ref.clone();
    Effect::new(move |_| {
        let ws_ref = ws_ref_clone.clone();
        spawn_local(async move {
            connect_websocket(ws_ref, set_events, set_ws_connected);
        });
    });

    // Derive tool calls from events
    let tool_calls = Memo::new(move |_| {
        events.get()
            .iter()
            .filter(|e| matches!(e, WsEvent::ToolStart { .. } | WsEvent::ToolResult { .. }))
            .cloned()
            .collect::<Vec<_>>()
    });

    // Derive thinking blocks from events
    let thinking_blocks = Memo::new(move |_| {
        events.get()
            .iter()
            .filter(|e| matches!(e, WsEvent::Thinking { .. }))
            .cloned()
            .collect::<Vec<_>>()
    });

    // Derive diffs from events
    let diffs = Memo::new(move |_| {
        events.get()
            .iter()
            .filter_map(|e| {
                if let WsEvent::DiffPreview { diff } = e {
                    Some(diff.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
    });

    view! {
        <Layout>
            <div class="max-w-6xl mx-auto">
                <div class="flex items-center justify-between mb-6">
                    <h1 class="text-2xl font-bold">"Ghost Mode"</h1>
                    <div class="flex items-center gap-2">
                        <div class=move || {
                            if ws_connected.get() {
                                "w-2 h-2 rounded-full bg-success animate-pulse"
                            } else {
                                "w-2 h-2 rounded-full bg-error"
                            }
                        }></div>
                        <span class="text-sm text-muted">
                            {move || if ws_connected.get() { "Live" } else { "Disconnected" }}
                        </span>
                        <button
                            class="ml-4 text-xs px-2 py-1 bg-card border border-border rounded hover:border-accent"
                            on:click=move |_| set_events.set(Vec::new())
                        >
                            "Clear"
                        </button>
                        <button
                            class="text-xs px-2 py-1 bg-card border border-border rounded hover:border-accent"
                            on:click=move |_| {
                                set_events.update(|events| {
                                    events.push(WsEvent::DiffPreview {
                                        diff: UnifiedDiff {
                                            file_path: "src/main.rs".to_string(),
                                            hunks: vec![DiffHunk {
                                                old_start: 10,
                                                old_lines: 5,
                                                new_start: 10,
                                                new_lines: 7,
                                                lines: vec![
                                                    DiffLine::Context("fn main() {".to_string()),
                                                    DiffLine::Remove("    println!(\"Hello\");".to_string()),
                                                    DiffLine::Add("    let name = \"World\";".to_string()),
                                                    DiffLine::Add("    println!(\"Hello, {}!\", name);".to_string()),
                                                    DiffLine::Context("}".to_string()),
                                                ],
                                            }],
                                        },
                                    });
                                });
                            }
                        >
                            "Test Diff"
                        </button>
                    </div>
                </div>

                <div class="grid grid-cols-3 gap-4">
                    // Thinking panel - 2 columns
                    <div class="col-span-2 bg-card rounded-lg border border-border p-4 min-h-96">
                        <h2 class="text-sm text-muted mb-4">"Agent Reasoning"</h2>
                        <ThinkingPanel thinking_blocks=thinking_blocks/>
                    </div>

                    // Tool timeline - 1 column
                    <div class="bg-card rounded-lg border border-border p-4 min-h-96 overflow-y-auto">
                        <h2 class="text-sm text-muted mb-4">"Tool Calls"</h2>
                        <ToolTimeline tool_calls=tool_calls/>
                    </div>
                </div>

                // Diff viewer
                <div class="mt-4 bg-card rounded-lg border border-border p-4">
                    <h2 class="text-sm text-muted mb-4">"File Changes"</h2>
                    <DiffViewer diffs=diffs/>
                </div>

                // Terminal output
                <div class="mt-4 bg-card rounded-lg border border-border p-4">
                    <h2 class="text-sm text-muted mb-4">"Terminal Output"</h2>
                    <TerminalMirror events=events/>
                </div>
            </div>
        </Layout>
    }
}

#[component]
fn ThinkingPanel(thinking_blocks: Memo<Vec<WsEvent>>) -> impl IntoView {
    let (expanded, set_expanded) = signal(true);

    view! {
        <div class="space-y-2">
            {move || {
                let blocks = thinking_blocks.get();
                if blocks.is_empty() {
                    view! {
                        <div class="text-muted italic">"Waiting for agent activity..."</div>
                    }.into_any()
                } else {
                    view! {
                        <div>
                            <button
                                class="text-xs text-muted hover:text-accent mb-2"
                                on:click=move |_| set_expanded.update(|e| *e = !*e)
                            >
                                {move || if expanded.get() { "[-] Collapse" } else { "[+] Expand" }}
                            </button>
                            <div class=move || if expanded.get() { "" } else { "hidden" }>
                                <For
                                    each=move || blocks.clone()
                                    key=|e| format!("{:?}", e)
                                    children=move |event| {
                                        if let WsEvent::Thinking { content, phase } = event {
                                            view! {
                                                <div class="mb-2 p-2 bg-background rounded">
                                                    <span class="text-xs text-accent mr-2">
                                                        {format!("[{:?}]", phase)}
                                                    </span>
                                                    <span class="text-sm font-mono whitespace-pre-wrap">
                                                        {content}
                                                    </span>
                                                </div>
                                            }.into_any()
                                        } else {
                                            view! { <div></div> }.into_any()
                                        }
                                    }
                                />
                            </div>
                        </div>
                    }.into_any()
                }
            }}
        </div>
    }
}

#[component]
fn ToolTimeline(tool_calls: Memo<Vec<WsEvent>>) -> impl IntoView {
    view! {
        <div class="space-y-2">
            {move || {
                let calls = tool_calls.get();
                if calls.is_empty() {
                    view! {
                        <div class="text-muted italic">"No tool calls yet"</div>
                    }.into_any()
                } else {
                    view! {
                        <For
                            each=move || calls.clone()
                            key=|e| format!("{:?}", e)
                            children=move |event| {
                                match event {
                                    WsEvent::ToolStart { tool_name, call_id, .. } => {
                                        view! {
                                            <div class="tool-call">
                                                <div class="flex items-center gap-2">
                                                    <span class="text-accent">">"</span>
                                                    <span class="font-semibold">{tool_name}</span>
                                                </div>
                                                <div class="text-xs text-muted mt-1">{call_id}</div>
                                            </div>
                                        }.into_any()
                                    }
                                    WsEvent::ToolResult { tool_name, success, duration_ms, .. } => {
                                        let status_class = if success { "success" } else { "error" };
                                        view! {
                                            <div class=format!("tool-call {}", status_class)>
                                                <div class="flex items-center gap-2">
                                                    <span class=if success { "text-success" } else { "text-error" }>
                                                        {if success { "ok" } else { "err" }}
                                                    </span>
                                                    <span>{tool_name}</span>
                                                    <span class="text-xs text-muted ml-auto">
                                                        {format!("{}ms", duration_ms)}
                                                    </span>
                                                </div>
                                            </div>
                                        }.into_any()
                                    }
                                    _ => view! { <div></div> }.into_any()
                                }
                            }
                        />
                    }.into_any()
                }
            }}
        </div>
    }
}

#[component]
fn DiffViewer(diffs: Memo<Vec<UnifiedDiff>>) -> impl IntoView {
    view! {
        <div class="space-y-4">
            {move || {
                let diff_list = diffs.get();
                if diff_list.is_empty() {
                    view! {
                        <div class="text-muted italic">"No file changes to display"</div>
                    }.into_any()
                } else {
                    view! {
                        <For
                            each=move || diff_list.clone()
                            key=|d| d.file_path.clone()
                            children=move |diff| {
                                view! { <DiffBlock diff=diff/> }
                            }
                        />
                    }.into_any()
                }
            }}
        </div>
    }
}

#[component]
fn DiffBlock(diff: UnifiedDiff) -> impl IntoView {
    let file_path = diff.file_path.clone();
    let extension = get_extension(&file_path).to_string();

    view! {
        <div class="border border-border rounded">
            <div class="px-3 py-2 bg-background border-b border-border font-mono text-sm flex justify-between">
                <span>{file_path.clone()}</span>
                <span class="text-xs text-muted">{extension.clone()}</span>
            </div>
            <div class="p-2 font-mono text-xs overflow-x-auto">
                <For
                    each=move || diff.hunks.clone()
                    key=|h| format!("{}-{}", h.old_start, h.new_start)
                    children={
                        let ext = extension.clone();
                        move |hunk| {
                            let ext = ext.clone();
                            view! {
                                <div class="mb-2">
                                    <div class="text-muted">
                                        {format!("@@ -{},{} +{},{} @@",
                                            hunk.old_start, hunk.old_lines,
                                            hunk.new_start, hunk.new_lines)}
                                    </div>
                                    <For
                                        each=move || hunk.lines.clone()
                                        key=|l| format!("{:?}", l)
                                        children={
                                            let ext = ext.clone();
                                            move |line| {
                                                let ext = ext.clone();
                                                let (prefix, bg_class) = match &line {
                                                    DiffLine::Context(_) => (" ", "diff-context"),
                                                    DiffLine::Add(_) => ("+", "diff-add"),
                                                    DiffLine::Remove(_) => ("-", "diff-remove"),
                                                };
                                                let content = match &line {
                                                    DiffLine::Context(c)
                                                    | DiffLine::Add(c)
                                                    | DiffLine::Remove(c) => c.clone(),
                                                };
                                                let highlighted = highlight_line(&content, &ext);
                                                view! {
                                                    <div class=bg_class>
                                                        <span class="select-none text-muted">{prefix}</span>
                                                        <span inner_html=highlighted></span>
                                                    </div>
                                                }
                                            }
                                        }
                                    />
                                </div>
                            }
                        }
                    }
                />
            </div>
        </div>
    }
}

#[component]
fn TerminalMirror(events: ReadSignal<Vec<WsEvent>>) -> impl IntoView {
    let terminal_output = Memo::new(move |_| {
        events.get()
            .iter()
            .filter_map(|e| {
                if let WsEvent::TerminalOutput { content, is_stderr } = e {
                    Some((content.clone(), *is_stderr))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
    });

    view! {
        <div class="terminal max-h-64 overflow-y-auto">
            {move || {
                let output = terminal_output.get();
                if output.is_empty() {
                    view! {
                        <div class="text-muted italic">"No terminal output"</div>
                    }.into_any()
                } else {
                    view! {
                        <For
                            each=move || output.clone()
                            key=|(c, _)| c.clone()
                            children=move |(content, is_stderr)| {
                                view! {
                                    <div class=if is_stderr { "text-error" } else { "text-foreground" }>
                                        {content}
                                    </div>
                                }
                            }
                        />
                    }.into_any()
                }
            }}
        </div>
    }
}

// ============================================================================
// Memories Page
// ============================================================================

#[component]
fn MemoriesPage() -> impl IntoView {
    let (memories, set_memories) = signal(Vec::<MemoryFact>::new());
    let (search_query, set_search_query) = signal(String::new());
    let (loading, set_loading) = signal(false);
    let (trigger_search, set_trigger_search) = signal(0u32);

    // Load memories on mount
    Effect::new(move |_| {
        spawn_local(async move {
            set_loading.set(true);
            if let Ok(mems) = fetch_memories().await {
                set_memories.set(mems);
            }
            set_loading.set(false);
        });
    });

    // Search effect - runs when trigger_search changes
    Effect::new(move |prev: Option<u32>| {
        let current = trigger_search.get();
        if prev.is_some() && current > 0 {
            let query = search_query.get();
            if !query.is_empty() {
                spawn_local(async move {
                    set_loading.set(true);
                    if let Ok(results) = recall_memories(&query).await {
                        set_memories.set(results);
                    }
                    set_loading.set(false);
                });
            }
        }
        current
    });

    view! {
        <Layout>
            <div class="max-w-4xl mx-auto">
                <h1 class="text-2xl font-bold mb-6">"Memories"</h1>

                // Search bar
                <div class="mb-6 flex gap-2">
                    <input
                        type="text"
                        placeholder="Search memories semantically..."
                        class="flex-1 p-3 bg-card border border-border rounded-lg focus:border-accent outline-none"
                        prop:value=move || search_query.get()
                        on:input=move |ev| {
                            set_search_query.set(event_target_value(&ev));
                        }
                        on:keydown=move |ev| {
                            if ev.key() == "Enter" {
                                set_trigger_search.update(|n| *n += 1);
                            }
                        }
                    />
                    <button
                        class="px-4 py-2 bg-accent text-background rounded-lg hover:opacity-90"
                        on:click=move |_| set_trigger_search.update(|n| *n += 1)
                    >
                        "Search"
                    </button>
                </div>

                // Loading state
                {move || loading.get().then(|| view! {
                    <div class="text-muted text-center py-4">"Loading..."</div>
                })}

                // Memory list
                <div class="space-y-4">
                    {move || {
                        let mems = memories.get();
                        if mems.is_empty() && !loading.get() {
                            view! {
                                <div class="text-muted italic text-center py-8">
                                    "No memories found"
                                </div>
                            }.into_any()
                        } else {
                            view! {
                                <For
                                    each=move || mems.clone()
                                    key=|m| m.id
                                    children=move |memory| {
                                        view! { <MemoryCard memory=memory/> }
                                    }
                                />
                            }.into_any()
                        }
                    }}
                </div>
            </div>
        </Layout>
    }
}

#[component]
fn MemoryCard(memory: MemoryFact) -> impl IntoView {
    view! {
        <div class="p-4 bg-card rounded-lg border border-border hover:border-accent transition-colors">
            <div class="flex gap-2 mb-2">
                <span class="badge badge-accent">{memory.fact_type.clone()}</span>
                {memory.category.clone().map(|cat| view! {
                    <span class="badge badge-muted">{cat}</span>
                })}
            </div>
            <p class="text-sm">{memory.content.clone()}</p>
            <div class="mt-2 text-xs text-muted">
                {memory.created_at.clone()}
            </div>
        </div>
    }
}

// ============================================================================
// Code Page
// ============================================================================

#[component]
fn CodePage() -> impl IntoView {
    let (results, set_results) = signal(Vec::<CodeSearchResult>::new());
    let (search_query, set_search_query) = signal(String::new());
    let (loading, set_loading) = signal(false);
    let (trigger_search, set_trigger_search) = signal(0u32);

    // Search effect - runs when trigger_search changes
    Effect::new(move |prev: Option<u32>| {
        let current = trigger_search.get();
        if prev.is_some() && current > 0 {
            let query = search_query.get();
            if !query.is_empty() {
                spawn_local(async move {
                    set_loading.set(true);
                    if let Ok(res) = search_code(&query).await {
                        set_results.set(res);
                    }
                    set_loading.set(false);
                });
            }
        }
        current
    });

    view! {
        <Layout>
            <div class="max-w-4xl mx-auto">
                <h1 class="text-2xl font-bold mb-6">"Code Intelligence"</h1>

                // Search bar
                <div class="mb-6 flex gap-2">
                    <input
                        type="text"
                        placeholder="Semantic code search..."
                        class="flex-1 p-3 bg-card border border-border rounded-lg focus:border-accent outline-none"
                        prop:value=move || search_query.get()
                        on:input=move |ev| {
                            set_search_query.set(event_target_value(&ev));
                        }
                        on:keydown=move |ev| {
                            if ev.key() == "Enter" {
                                set_trigger_search.update(|n| *n += 1);
                            }
                        }
                    />
                    <button
                        class="px-4 py-2 bg-accent text-background rounded-lg hover:opacity-90"
                        on:click=move |_| set_trigger_search.update(|n| *n += 1)
                    >
                        "Search"
                    </button>
                </div>

                // Loading
                {move || loading.get().then(|| view! {
                    <div class="text-muted text-center py-4">"Searching..."</div>
                })}

                // Results
                <div class="space-y-4">
                    {move || {
                        let res = results.get();
                        if res.is_empty() && !loading.get() {
                            view! {
                                <div class="text-muted italic text-center py-8">
                                    "Enter a query to search code semantically"
                                </div>
                            }.into_any()
                        } else {
                            view! {
                                <For
                                    each=move || res.clone()
                                    key=|r| format!("{}:{}", r.file_path, r.line_number)
                                    children=move |result| {
                                        view! { <CodeResult result=result/> }
                                    }
                                />
                            }.into_any()
                        }
                    }}
                </div>
            </div>
        </Layout>
    }
}

#[component]
fn CodeResult(result: CodeSearchResult) -> impl IntoView {
    view! {
        <div class="p-4 bg-card rounded-lg border border-border">
            <div class="flex items-center gap-2 mb-2">
                <span class="font-mono text-accent">{result.file_path.clone()}</span>
                <span class="text-muted">":"</span>
                <span class="text-muted">{result.line_number}</span>
                {result.symbol_name.clone().map(|name| view! {
                    <span class="ml-2 badge badge-accent">{name}</span>
                })}
            </div>
            <pre class="text-sm font-mono bg-background p-2 rounded overflow-x-auto">
                {result.content.clone()}
            </pre>
        </div>
    }
}

// ============================================================================
// Tasks Page
// ============================================================================

#[component]
fn TasksPage() -> impl IntoView {
    let (goals, set_goals) = signal(Vec::<Goal>::new());
    let (tasks, set_tasks) = signal(Vec::<Task>::new());
    let (loading, set_loading) = signal(false);

    // Load on mount
    Effect::new(move |_| {
        spawn_local(async move {
            set_loading.set(true);
            if let Ok(g) = fetch_goals().await {
                set_goals.set(g);
            }
            if let Ok(t) = fetch_tasks().await {
                set_tasks.set(t);
            }
            set_loading.set(false);
        });
    });

    view! {
        <Layout>
            <div class="max-w-4xl mx-auto">
                <h1 class="text-2xl font-bold mb-6">"Tasks & Goals"</h1>

                {move || loading.get().then(|| view! {
                    <div class="text-muted text-center py-4">"Loading..."</div>
                })}

                <div class="grid grid-cols-2 gap-6">
                    // Goals column
                    <div>
                        <h2 class="text-lg font-semibold mb-4">"Goals"</h2>
                        <div class="space-y-2">
                            {move || {
                                let g = goals.get();
                                if g.is_empty() {
                                    view! {
                                        <div class="text-muted italic">"No goals yet"</div>
                                    }.into_any()
                                } else {
                                    view! {
                                        <For
                                            each=move || g.clone()
                                            key=|goal| goal.id
                                            children=move |goal| {
                                                view! { <GoalCard goal=goal/> }
                                            }
                                        />
                                    }.into_any()
                                }
                            }}
                        </div>
                    </div>

                    // Tasks column
                    <div>
                        <h2 class="text-lg font-semibold mb-4">"Tasks"</h2>
                        <div class="space-y-2">
                            {move || {
                                let t = tasks.get();
                                if t.is_empty() {
                                    view! {
                                        <div class="text-muted italic">"No tasks yet"</div>
                                    }.into_any()
                                } else {
                                    view! {
                                        <For
                                            each=move || t.clone()
                                            key=|task| task.id
                                            children=move |task| {
                                                view! { <TaskCard task=task/> }
                                            }
                                        />
                                    }.into_any()
                                }
                            }}
                        </div>
                    </div>
                </div>
            </div>
        </Layout>
    }
}

#[component]
fn GoalCard(goal: Goal) -> impl IntoView {
    let status_str = goal.status.as_str();
    let priority_str = goal.priority.as_str();

    let status_class = match goal.status {
        GoalStatus::Completed => "text-success",
        GoalStatus::InProgress => "text-accent",
        GoalStatus::Blocked => "text-error",
        _ => "text-muted",
    };

    view! {
        <div class="p-3 bg-card rounded border border-border">
            <div class="flex items-center gap-2">
                <span class=status_class>">"</span>
                <span class="font-semibold">{goal.title.clone()}</span>
            </div>
            {goal.description.clone().map(|desc| view! {
                <p class="text-sm text-muted mt-1">{desc}</p>
            })}
            <div class="mt-2 flex gap-2">
                <span class="text-xs badge badge-muted">{status_str}</span>
                <span class="text-xs badge badge-muted">{priority_str}</span>
            </div>
        </div>
    }
}

#[component]
fn TaskCard(task: Task) -> impl IntoView {
    let status_icon = match task.status {
        TaskStatus::Completed => "[x]",
        TaskStatus::InProgress => "[~]",
        TaskStatus::Blocked => "[!]",
        TaskStatus::Pending => "[ ]",
    };
    let status_class = match task.status {
        TaskStatus::Completed => "text-success",
        TaskStatus::InProgress => "text-accent",
        TaskStatus::Blocked => "text-error",
        TaskStatus::Pending => "text-muted",
    };

    view! {
        <div class="p-3 bg-card rounded border border-border">
            <div class="flex items-center gap-2">
                <span class=format!("font-mono {}", status_class)>{status_icon}</span>
                <span>{task.title.clone()}</span>
            </div>
            {task.description.clone().map(|desc| view! {
                <p class="text-sm text-muted mt-1">{desc}</p>
            })}
        </div>
    }
}

// ============================================================================
// API Functions
// ============================================================================

async fn fetch_health() -> Result<String, String> {
    let window = web_sys::window().ok_or("No window")?;
    let location = window.location();
    let host = location.host().map_err(|_| "No host")?;
    let protocol = location.protocol().map_err(|_| "No protocol")?;

    let url = format!("{}//{}/api/health", protocol, host);

    let resp = gloo_net::http::Request::get(&url)
        .send()
        .await
        .map_err(|e| format!("Fetch error: {:?}", e))?;

    resp.text()
        .await
        .map_err(|e| format!("Text error: {:?}", e))
}

async fn fetch_memories() -> Result<Vec<MemoryFact>, String> {
    let url = get_api_url("/api/memories");
    let resp = gloo_net::http::Request::get(&url)
        .send()
        .await
        .map_err(|e| format!("{:?}", e))?;

    #[derive(Deserialize)]
    struct ApiResponse {
        data: Vec<MemoryFact>,
    }

    let data: ApiResponse = resp.json().await.map_err(|e| format!("{:?}", e))?;
    Ok(data.data)
}

async fn recall_memories(query: &str) -> Result<Vec<MemoryFact>, String> {
    let url = get_api_url("/api/recall");

    #[derive(Serialize)]
    struct RecallReq {
        query: String,
        limit: Option<u32>,
    }

    let resp = gloo_net::http::Request::post(&url)
        .json(&RecallReq { query: query.to_string(), limit: Some(20) })
        .map_err(|e| format!("{:?}", e))?
        .send()
        .await
        .map_err(|e| format!("{:?}", e))?;

    #[derive(Deserialize)]
    struct ApiResponse {
        data: Vec<MemoryFact>,
    }

    let data: ApiResponse = resp.json().await.map_err(|e| format!("{:?}", e))?;
    Ok(data.data)
}

async fn search_code(query: &str) -> Result<Vec<CodeSearchResult>, String> {
    let url = get_api_url("/api/search/code");

    #[derive(Serialize)]
    struct SearchReq {
        query: String,
        limit: Option<u32>,
    }

    let resp = gloo_net::http::Request::post(&url)
        .json(&SearchReq { query: query.to_string(), limit: Some(20) })
        .map_err(|e| format!("{:?}", e))?
        .send()
        .await
        .map_err(|e| format!("{:?}", e))?;

    #[derive(Deserialize)]
    struct ApiResponse {
        data: Vec<CodeSearchResult>,
    }

    let data: ApiResponse = resp.json().await.map_err(|e| format!("{:?}", e))?;
    Ok(data.data)
}

async fn fetch_goals() -> Result<Vec<Goal>, String> {
    let url = get_api_url("/api/goals");
    let resp = gloo_net::http::Request::get(&url)
        .send()
        .await
        .map_err(|e| format!("{:?}", e))?;

    #[derive(Deserialize)]
    struct ApiResponse {
        data: Vec<Goal>,
    }

    let data: ApiResponse = resp.json().await.map_err(|e| format!("{:?}", e))?;
    Ok(data.data)
}

async fn fetch_tasks() -> Result<Vec<Task>, String> {
    let url = get_api_url("/api/tasks");
    let resp = gloo_net::http::Request::get(&url)
        .send()
        .await
        .map_err(|e| format!("{:?}", e))?;

    #[derive(Deserialize)]
    struct ApiResponse {
        data: Vec<Task>,
    }

    let data: ApiResponse = resp.json().await.map_err(|e| format!("{:?}", e))?;
    Ok(data.data)
}

fn get_api_url(path: &str) -> String {
    let window = web_sys::window().expect("No window");
    let location = window.location();
    let host = location.host().expect("No host");
    let protocol = location.protocol().expect("No protocol");
    format!("{}//{}{}", protocol, host, path)
}

// ============================================================================
// WebSocket Functions
// ============================================================================

/// Extract event ID from call_id (format: "replay-{id}" for replayed events)
fn parse_event_id(call_id: &str) -> Option<i64> {
    call_id.strip_prefix("replay-").and_then(|id| id.parse().ok())
}

fn connect_websocket(
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

// CodeSearchResult is now defined in mira_types

// Global WebSocket connection (just for connection status, no event handling)
fn connect_websocket_global(
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
