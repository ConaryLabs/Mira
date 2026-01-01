// src/web/components/mod.rs
// Leptos SSR components for Mira Studio

use leptos::prelude::*;
use leptos_meta::*;
use leptos_router::components::*;
use leptos_router::path;

/// Shell component that wraps the entire app
#[component]
pub fn Shell(children: Children) -> impl IntoView {
    provide_meta_context();

    view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8"/>
                <meta name="viewport" content="width=device-width, initial-scale=1"/>
                <Title text="Mira Studio"/>
                <Meta name="description" content="Memory and Intelligence Layer for Claude Code"/>
                <Stylesheet href="/assets/style.css"/>
            </head>
            <body class="bg-background text-foreground font-mono">
                {children()}
            </body>
        </html>
    }
}

/// Main app component with routing
#[component]
pub fn App() -> impl IntoView {
    view! {
        <Router>
            <Shell>
                <Routes fallback=|| "Page not found">
                    <Route path=path!("/") view=HomePage/>
                    <Route path=path!("/ghost") view=GhostModePage/>
                    <Route path=path!("/memories") view=MemoriesPage/>
                    <Route path=path!("/code") view=CodePage/>
                    <Route path=path!("/tasks") view=TasksPage/>
                </Routes>
            </Shell>
        </Router>
    }
}

/// Layout wrapper with nav
#[component]
pub fn Layout(children: Children) -> impl IntoView {
    view! {
        <div class="min-h-screen flex flex-col">
            <Nav/>
            <main class="flex-1 p-4">
                {children()}
            </main>
        </div>
    }
}

/// Navigation component
#[component]
pub fn Nav() -> impl IntoView {
    view! {
        <nav class="border-b border-border px-4 py-3 flex items-center gap-6">
            <a href="/" class="text-accent font-bold text-lg">"Mira Studio"</a>
            <div class="flex gap-4 text-sm">
                <a href="/ghost" class="hover:text-accent">"Ghost Mode"</a>
                <a href="/memories" class="hover:text-accent">"Memories"</a>
                <a href="/code" class="hover:text-accent">"Code"</a>
                <a href="/tasks" class="hover:text-accent">"Tasks"</a>
            </div>
        </nav>
    }
}

/// Home page
#[component]
pub fn HomePage() -> impl IntoView {
    view! {
        <Layout>
            <div class="max-w-4xl mx-auto py-12 text-center">
                <h1 class="text-4xl font-bold text-accent mb-4">"Mira Studio"</h1>
                <p class="text-muted mb-8">"Memory and Intelligence Layer for Claude Code"</p>

                <div class="grid grid-cols-2 gap-4 max-w-2xl mx-auto">
                    <Card title="Ghost Mode" href="/ghost" icon="eye">
                        "Real-time agent reasoning visualization"
                    </Card>
                    <Card title="Memories" href="/memories" icon="brain">
                        "Semantic memory storage and search"
                    </Card>
                    <Card title="Code Intel" href="/code" icon="code">
                        "Code symbols and semantic search"
                    </Card>
                    <Card title="Tasks" href="/tasks" icon="check">
                        "Goals and task management"
                    </Card>
                </div>

                <div class="mt-12 p-4 bg-card rounded-lg border border-border">
                    <h3 class="text-sm text-muted mb-2">"Server Status"</h3>
                    <p class="text-success">"Connected to Mira backend"</p>
                </div>
            </div>
        </Layout>
    }
}

/// Card component for home page
#[allow(unused_variables)]
#[component]
fn Card(
    title: &'static str,
    href: &'static str,
    #[prop(into, optional)]
    icon: Option<&'static str>,
    children: Children,
) -> impl IntoView {
    view! {
        <a href=href class="block p-6 bg-card rounded-lg border border-border hover:border-accent transition-colors">
            <h3 class="text-lg font-semibold mb-2">{title}</h3>
            <p class="text-sm text-muted">{children()}</p>
        </a>
    }
}

/// Ghost Mode page - agent reasoning visualization
#[component]
pub fn GhostModePage() -> impl IntoView {
    view! {
        <Layout>
            <div class="max-w-6xl mx-auto">
                <h1 class="text-2xl font-bold mb-6">"Ghost Mode"</h1>

                <div class="grid grid-cols-3 gap-4">
                    // Thinking panel
                    <div class="col-span-2 bg-card rounded-lg border border-border p-4">
                        <h2 class="text-sm text-muted mb-4">"Agent Reasoning"</h2>
                        <ThinkingPanel/>
                    </div>

                    // Tool timeline
                    <div class="bg-card rounded-lg border border-border p-4">
                        <h2 class="text-sm text-muted mb-4">"Tool Calls"</h2>
                        <ToolTimeline/>
                    </div>
                </div>

                // Diff viewer
                <div class="mt-4 bg-card rounded-lg border border-border p-4">
                    <h2 class="text-sm text-muted mb-4">"File Changes"</h2>
                    <DiffViewer/>
                </div>
            </div>
        </Layout>
    }
}

/// Thinking panel with accordion
#[component]
fn ThinkingPanel() -> impl IntoView {
    view! {
        <div class="space-y-2">
            <div class="text-muted italic">"Waiting for agent activity..."</div>
            // Will be populated by WebSocket events
        </div>
    }
}

/// Tool call timeline
#[component]
fn ToolTimeline() -> impl IntoView {
    view! {
        <div class="space-y-2">
            <div class="text-muted italic">"No tool calls yet"</div>
            // Will be populated by WebSocket events
        </div>
    }
}

/// Diff viewer
#[component]
fn DiffViewer() -> impl IntoView {
    view! {
        <div class="text-muted italic">"No file changes to display"</div>
    }
}

/// Memories page
#[component]
pub fn MemoriesPage() -> impl IntoView {
    view! {
        <Layout>
            <div class="max-w-4xl mx-auto">
                <h1 class="text-2xl font-bold mb-6">"Memories"</h1>

                // Search bar
                <div class="mb-6">
                    <input
                        type="text"
                        placeholder="Search memories..."
                        class="w-full p-3 bg-card border border-border rounded-lg focus:border-accent outline-none"
                    />
                </div>

                // Memory list
                <div class="space-y-4">
                    <MemoryCard
                        content="Leptos Studio plan created 2025-12-31..."
                        fact_type="decision"
                        category="architecture"
                    />
                </div>
            </div>
        </Layout>
    }
}

/// Memory card component
#[component]
fn MemoryCard(
    content: &'static str,
    fact_type: &'static str,
    category: &'static str,
) -> impl IntoView {
    view! {
        <div class="p-4 bg-card rounded-lg border border-border">
            <div class="flex gap-2 mb-2">
                <span class="text-xs px-2 py-1 bg-accent/20 text-accent rounded">{fact_type}</span>
                <span class="text-xs px-2 py-1 bg-muted/20 text-muted rounded">{category}</span>
            </div>
            <p class="text-sm">{content}</p>
        </div>
    }
}

/// Code page
#[component]
pub fn CodePage() -> impl IntoView {
    view! {
        <Layout>
            <div class="max-w-4xl mx-auto">
                <h1 class="text-2xl font-bold mb-6">"Code Intelligence"</h1>

                // Search bar
                <div class="mb-6">
                    <input
                        type="text"
                        placeholder="Semantic code search..."
                        class="w-full p-3 bg-card border border-border rounded-lg focus:border-accent outline-none"
                    />
                </div>

                <div class="text-muted">"Enter a query to search code semantically"</div>
            </div>
        </Layout>
    }
}

/// Tasks page
#[component]
pub fn TasksPage() -> impl IntoView {
    view! {
        <Layout>
            <div class="max-w-4xl mx-auto">
                <h1 class="text-2xl font-bold mb-6">"Tasks & Goals"</h1>

                <div class="grid grid-cols-2 gap-6">
                    // Goals
                    <div>
                        <h2 class="text-lg font-semibold mb-4">"Goals"</h2>
                        <div class="space-y-2">
                            <div class="text-muted italic">"No goals yet"</div>
                        </div>
                    </div>

                    // Tasks
                    <div>
                        <h2 class="text-lg font-semibold mb-4">"Tasks"</h2>
                        <div class="space-y-2">
                            <div class="text-muted italic">"No tasks yet"</div>
                        </div>
                    </div>
                </div>
            </div>
        </Layout>
    }
}
