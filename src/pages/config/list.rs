/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use std::sync::Arc;

use leptos::*;
use leptos_router::*;

use crate::{
    components::{
        icon::{IconAdd, IconRefresh, IconTrash},
        list::{
            header::ColumnList,
            pagination::Pagination,
            row::SelectItem,
            toolbar::{SearchBox, ToolbarButton},
            Footer, ItemSelection, ListItem, ListSection, ListTable, ListTextItem, Toolbar,
            ZeroResults,
        },
        messages::{
            alert::{use_alerts, Alert},
            modal::{use_modals, Modal},
        },
        skeleton::Skeleton,
        Color,
    },
    core::{
        http::{self, HttpRequest},
        oauth::use_authorization,
        url::UrlBuilder,
    },
    pages::{
        config::{ReloadSettings, SchemaType, Schemas, SettingsValues},
        maybe_plural, List,
    },
};

use super::{Schema, Settings, UpdateSettings};

#[component]
pub fn SettingsList() -> impl IntoView {
    let schemas = expect_context::<Arc<Schemas>>();
    let query = use_query_map();
    let page = create_memo(move |_| {
        query
            .with(|q| q.get("page").and_then(|page| page.parse::<u32>().ok()))
            .filter(|&page| page > 0)
            .unwrap_or(1)
    });
    let filter = create_memo(move |_| {
        query.with(|q| {
            q.get("filter").and_then(|s| {
                let s = s.trim();
                if !s.is_empty() {
                    Some(s.to_string())
                } else {
                    None
                }
            })
        })
    });
    let selected = create_rw_signal::<ItemSelection>(ItemSelection::None);
    let params = use_params_map();
    let current_schema = create_memo(move |_| {
        if let Some(schema) = params
            .get()
            .get("object")
            .and_then(|id| schemas.schemas.get(id.as_str()))
        {
            selected.set(ItemSelection::None);
            schema.clone()
        } else {
            use_navigate()("/404", Default::default());
            Arc::new(Schema::default())
        }
    });

    let auth = use_authorization();
    let alert = use_alerts();
    let modal = use_modals();
    provide_context(selected);

    let settings = create_resource(
        move || (page.get(), filter.get()),
        move |(page, filter)| {
            let auth = auth.get_untracked();
            let schema = current_schema.get();

            async move {
                HttpRequest::get("/api/settings/group")
                    .with_authorization(&auth)
                    .with_parameter("page", page.to_string())
                    .with_parameter("limit", schema.list.page_size.to_string())
                    .with_parameter("prefix", schema.unwrap_prefix())
                    .with_parameter("suffix", schema.try_unwrap_suffix().unwrap_or_default())
                    .with_optional_parameter("filter", filter)
                    .send::<List<Settings>>()
                    .await
            }
        },
    );

    let reload_config_action = create_action(move |()| {
        let schema = current_schema.get();
        let auth = auth.get();

        async move {
            match HttpRequest::get(format!(
                "/api/reload/{}",
                schema.reload_prefix.unwrap_or_default()
            ))
            .with_authorization(&auth)
            .send::<ReloadSettings>()
            .await
            {
                Ok(result) => {
                    alert.set(Alert::from(result));
                }
                Err(http::Error::Unauthorized) => {
                    use_navigate()("/login", Default::default());
                }
                Err(err) => {
                    alert.set(Alert::from(err));
                }
            }
        }
    });

    let total_results = create_rw_signal(None::<u32>);
    let delete_action = create_action(move |items: &Arc<ItemSelection>| {
        let items = items.clone();
        let auth = auth.get();
        let schema = current_schema.get();
        let filter = filter.get();

        async move {
            let updates = match items.as_ref() {
                ItemSelection::All => {
                    vec![match schema.typ {
                        SchemaType::Record { prefix, .. } | SchemaType::Entry { prefix } => {
                            UpdateSettings::Clear {
                                prefix: format!("{prefix}."),
                                filter,
                            }
                        }
                        SchemaType::List => panic!("List schema type is not supported."),
                    }]
                }
                ItemSelection::Some(items) => {
                    let mut updates = Vec::with_capacity(items.len());
                    for item in items.iter() {
                        if !item.is_empty() {
                            match schema.typ {
                                SchemaType::Record { prefix, .. } => {
                                    updates.push(UpdateSettings::Clear {
                                        prefix: format!("{prefix}.{item}."),
                                        filter: None,
                                    });
                                }
                                SchemaType::Entry { prefix } => {
                                    updates.push(UpdateSettings::Delete {
                                        keys: vec![format!("{prefix}.{item}")],
                                    });
                                }
                                SchemaType::List => panic!("List schema type is not supported."),
                            }
                        }
                    }
                    updates
                }
                ItemSelection::None => unreachable!(),
            };

            match HttpRequest::post("/api/settings")
                .with_authorization(&auth)
                .with_body(updates)
                .unwrap()
                .send::<serde_json::Value>()
                .await
            {
                Ok(_) => {
                    settings.refetch();
                    alert.set(Alert::success(format!(
                        "Deleted {}.",
                        maybe_plural(
                            items.total_selected(total_results.get_untracked()),
                            schema.name_singular,
                            schema.name_plural,
                        )
                    )));
                }
                Err(err) => {
                    alert.set(Alert::from(err));
                }
            }
        }
    });

    view! {
        <ListSection>
            <ListTable
                title=Signal::derive(move || { current_schema.get().list.title.to_string() })
                subtitle=Signal::derive(move || { current_schema.get().list.subtitle.to_string() })
            >
                <Toolbar slot>
                    <SearchBox
                        value=filter
                        on_search=move |value| {
                            use_navigate()(
                                &UrlBuilder::new("/settings")
                                    .with_subpath(current_schema.get().id)
                                    .with_parameter("filter", value)
                                    .finish(),
                                Default::default(),
                            );
                        }
                    />

                    <ToolbarButton
                        text=Signal::derive(move || {
                            let ns = selected.get().total_selected(total_results.get());
                            if ns > 0 { format!("Delete ({ns})") } else { "Delete".to_string() }
                        })

                        color=Color::Red
                        on_click=Callback::new(move |_| {
                            let to_delete = selected.get().total_selected(total_results.get());
                            if to_delete > 0 {
                                let schema = current_schema.get();
                                let text = maybe_plural(
                                    to_delete,
                                    schema.name_singular,
                                    schema.name_plural,
                                );
                                modal
                                    .set(
                                        Modal::with_title("Confirm deletion")
                                            .with_message(
                                                format!(
                                                    "Are you sure you want to delete {text}? This action cannot be undone.",
                                                ),
                                            )
                                            .with_button(format!("Delete {text}"))
                                            .with_dangerous_callback(move || {
                                                delete_action
                                                    .dispatch(
                                                        Arc::new(
                                                            selected.try_update(std::mem::take).unwrap_or_default(),
                                                        ),
                                                    );
                                            }),
                                    )
                            }
                        })
                    >

                        <IconTrash/>
                    </ToolbarButton>

                    <ToolbarButton
                        text="Reload config"

                        color=Color::Gray
                        on_click=Callback::new(move |_| {
                            reload_config_action.dispatch(());
                        })
                    >

                        <IconRefresh/>
                    </ToolbarButton>

                    <ToolbarButton
                        text=Signal::derive(move || {
                            format!("Create {}", current_schema.get().name_singular)
                        })

                        color=Color::Blue
                        on_click=move |_| {
                            use_navigate()(
                                &format!("/settings/{}/edit", current_schema.get().id),
                                Default::default(),
                            );
                        }
                    >

                        <IconAdd size=16 attr:class="flex-shrink-0 size-3"/>
                    </ToolbarButton>

                </Toolbar>

                <Transition fallback=Skeleton>
                    {move || match settings.get() {
                        None => None,
                        Some(Err(http::Error::Unauthorized)) => {
                            use_navigate()("/login", Default::default());
                            Some(view! { <div></div> }.into_view())
                        }
                        Some(Err(err)) => {
                            total_results.set(Some(0));
                            alert.set(Alert::from(err));
                            Some(view! { <Skeleton/> }.into_view())
                        }
                        Some(Ok(settings)) if !settings.items.is_empty() => {
                            total_results.set(Some(settings.total as u32));
                            let schema = current_schema.get();
                            let mut headers = schema
                                .list
                                .fields
                                .iter()
                                .map(|f| f.label_column.to_string())
                                .collect::<Vec<_>>();
                            if schema.can_edit() {
                                headers.push("".to_string());
                            }
                            Some(
                                view! {
                                    <ColumnList headers=headers has_select_all=true>

                                        <For
                                            each=move || settings.items.clone()
                                            key=|setting| {
                                                setting
                                                    .get("_id")
                                                    .map(|s| s.to_string())
                                                    .unwrap_or_default()
                                            }

                                            let:settings
                                        >
                                            <SettingsItem settings schema=schema.clone()/>
                                        </For>

                                    </ColumnList>
                                }
                                    .into_view(),
                            )
                        }
                        Some(Ok(_)) => {
                            total_results.set(Some(0));
                            Some(
                                view! {
                                    <ZeroResults
                                        title="No results"
                                        subtitle="Your search did not yield any results."
                                        button_text=Signal::derive(move || {
                                            format!(
                                                "Create a new {}",
                                                current_schema.get().name_singular,
                                            )
                                        })

                                        button_action=Callback::new(move |_| {
                                            use_navigate()(
                                                &format!("/settings/{}/edit", current_schema.get().id),
                                                Default::default(),
                                            );
                                        })
                                    />
                                }
                                    .into_view(),
                            )
                        }
                    }}

                </Transition>

                <Footer slot>

                    <Pagination
                        current_page=page
                        total_results=total_results.read_only()
                        page_size=Signal::derive(move || current_schema.get().list.page_size)
                        on_page_change=move |page: u32| {
                            use_navigate()(
                                &UrlBuilder::new("/settings")
                                    .with_subpath(current_schema.get().id)
                                    .with_parameter("page", page.to_string())
                                    .with_optional_parameter("filter", filter.get())
                                    .finish(),
                                Default::default(),
                            );
                        }
                    />

                </Footer>
            </ListTable>
        </ListSection>
    }
}

#[component]
fn SettingsItem(settings: Settings, schema: Arc<Schema>) -> impl IntoView {
    let columns = schema
        .list
        .fields
        .iter()
        .map(|field| {
            let value = settings.format(field);
            view! { <ListTextItem>{value}</ListTextItem> }
        })
        .collect_view();
    let setting_id = settings
        .get("_id")
        .map(|s| s.to_string())
        .unwrap_or_default();
    let edit_link = if schema.can_edit() {
        let edit_url = format!("/settings/{}/{}/edit", schema.id, setting_id);
        Some(view! {
            <ListItem subclass="px-6 py-1.5">
                <a
                    class="inline-flex items-center gap-x-1 text-sm text-blue-600 decoration-2 hover:underline font-medium dark:focus:outline-none dark:focus:ring-1 dark:focus:ring-gray-600"
                    href=edit_url
                >
                    Edit
                </a>
            </ListItem>
        })
    } else {
        None
    };

    view! {
        <tr>
            <ListItem>
                <label class="flex">
                    <SelectItem item_id=setting_id/>

                    <span class="sr-only">Checkbox</span>
                </label>
            </ListItem>
            {columns}
            {edit_link}

        </tr>
    }
}
