// crates/mira-app/src/pages/ghost.rs
// Ghost Mode page - real-time agent reasoning visualization

use leptos::prelude::*;
use leptos::html;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen_futures::spawn_local;
use web_sys::WebSocket;
use mira_types::{WsEvent, UnifiedDiff, DiffHunk, DiffLine};
use crate::ansi::ansi_to_html;
use crate::syntax::{highlight_line, get_extension};
use crate::websocket::connect_websocket;
use crate::Layout;

/// Maximum lines to keep in terminal scrollback buffer
const TERMINAL_SCROLLBACK_LIMIT: usize = 1000;

#[component]
pub fn GhostModePage() -> impl IntoView {
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
    // Process terminal output with ANSI parsing and scrollback limit
    let terminal_lines = Memo::new(move |_| {
        let mut lines: Vec<(String, bool)> = Vec::new();

        for event in events.get().iter() {
            if let WsEvent::TerminalOutput { content, is_stderr } = event {
                // Split content into lines and add each
                for line in content.lines() {
                    lines.push((line.to_string(), *is_stderr));
                }
                // Handle trailing newline - if content ends with newline, we've already added all lines
                // If content doesn't end with newline, the last line is partial (will be continued)
            }
        }

        // Apply scrollback limit - keep only the last N lines
        if lines.len() > TERMINAL_SCROLLBACK_LIMIT {
            lines = lines.split_off(lines.len() - TERMINAL_SCROLLBACK_LIMIT);
        }

        lines
    });

    // Reference for auto-scroll
    let terminal_ref = NodeRef::<html::Div>::new();

    // Auto-scroll to bottom when new output arrives
    Effect::new(move |_| {
        let _ = terminal_lines.get(); // Subscribe to changes
        if let Some(el) = terminal_ref.get() {
            // Scroll to bottom
            el.set_scroll_top(el.scroll_height());
        }
    });

    view! {
        <div
            node_ref=terminal_ref
            class="terminal max-h-64 overflow-y-auto font-mono text-sm"
        >
            {move || {
                let lines = terminal_lines.get();
                if lines.is_empty() {
                    view! {
                        <div class="text-muted italic">"No terminal output"</div>
                    }.into_any()
                } else {
                    view! {
                        <div class="space-y-0">
                            <For
                                each={move || lines.clone().into_iter().enumerate().collect::<Vec<_>>()}
                                key={|(idx, (content, _))| format!("{}-{}", idx, content)}
                                children={move |(_, (content, is_stderr))| {
                                    let html_content = ansi_to_html(&content);
                                    let base_class = if is_stderr {
                                        "terminal-line terminal-stderr"
                                    } else {
                                        "terminal-line"
                                    };
                                    view! {
                                        <div class=base_class inner_html=html_content></div>
                                    }
                                }}
                            />
                        </div>
                    }.into_any()
                }
            }}
        </div>
    }
}
