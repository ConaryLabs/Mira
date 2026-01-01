// crates/mira-app/src/pages/tasks.rs
// Tasks and Goals page

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;
use mira_types::{Goal, Task, GoalStatus, TaskStatus};
use crate::api::{fetch_goals, fetch_tasks};
use crate::Layout;

#[component]
pub fn TasksPage() -> impl IntoView {
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
