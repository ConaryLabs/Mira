// crates/mira-app/src/components/sidebar.rs
// Project sidebar component

use leptos::prelude::*;
use mira_types::ProjectContext;

#[component]
pub fn ProjectSidebar(
    open: RwSignal<bool>,
    projects: Signal<Vec<ProjectContext>>,
    current_project: Signal<Option<ProjectContext>>,
    on_select: impl Fn(Option<ProjectContext>) + 'static + Clone + Send,
) -> impl IntoView {
    let on_select_clone = on_select.clone();
    let (filter, set_filter) = signal(String::new());

    let filtered_projects = move || {
        let query = filter.get().to_lowercase();
        if query.is_empty() {
            projects.get()
        } else {
            projects.get()
                .into_iter()
                .filter(|p| {
                    p.name.as_deref().unwrap_or("").to_lowercase().contains(&query)
                        || p.path.to_lowercase().contains(&query)
                })
                .collect()
        }
    };

    let close = move |_| open.set(false);

    view! {
        // Backdrop
        <div
            class=move || if open.get() { "sidebar-backdrop open" } else { "sidebar-backdrop" }
            on:click=close.clone()
        ></div>

        // Sidebar
        <div class=move || if open.get() { "sidebar open" } else { "sidebar" }>
            <div class="sidebar-header">
                <span class="sidebar-title">"Projects"</span>
                <button class="sidebar-close" on:click=close>
                    <svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor">
                        <path d="M4.646 4.646a.5.5 0 0 1 .708 0L8 7.293l2.646-2.647a.5.5 0 0 1 .708.708L8.707 8l2.647 2.646a.5.5 0 0 1-.708.708L8 8.707l-2.646 2.647a.5.5 0 0 1-.708-.708L7.293 8 4.646 5.354a.5.5 0 0 1 0-.708z"/>
                    </svg>
                </button>
            </div>

            <div class="sidebar-search">
                <input
                    type="text"
                    placeholder="Filter projects..."
                    prop:value=filter
                    on:input=move |ev| set_filter.set(event_target_value(&ev))
                />
            </div>

            <div class="sidebar-list">
                // "No Project" option
                {
                    let on_select = on_select.clone();
                    let is_selected = move || current_project.get().is_none();
                    view! {
                        <div
                            class=move || if is_selected() { "sidebar-item selected" } else { "sidebar-item" }
                            on:click={
                                let on_select = on_select.clone();
                                move |_| {
                                    on_select(None);
                                    open.set(false);
                                }
                            }
                        >
                            <div class="sidebar-item-name text-muted">"No Project"</div>
                            <div class="sidebar-item-path">"Global context only"</div>
                        </div>
                    }
                }

                // Project list
                <For
                    each=filtered_projects
                    key=|p| p.id
                    children={
                        let on_select = on_select_clone.clone();
                        move |project: ProjectContext| {
                            let project_clone = project.clone();
                            let on_select = on_select.clone();
                            let is_selected = {
                                let project_id = project.id;
                                move || {
                                    current_project.get()
                                        .map(|p| p.id == project_id)
                                        .unwrap_or(false)
                                }
                            };
                            view! {
                                <div
                                    class=move || if is_selected() { "sidebar-item selected" } else { "sidebar-item" }
                                    on:click={
                                        let project = project_clone.clone();
                                        let on_select = on_select.clone();
                                        move |_| {
                                            on_select(Some(project.clone()));
                                            open.set(false);
                                        }
                                    }
                                >
                                    <div class="sidebar-item-name">{project.name.clone().unwrap_or_else(|| "Unnamed".to_string())}</div>
                                    <div class="sidebar-item-path">{project.path.clone()}</div>
                                </div>
                            }
                        }
                    }
                />
            </div>
        </div>
    }
}
