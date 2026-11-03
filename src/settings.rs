use dioxus::prelude::*;
use noctics_shared::{BuildError, ClaudeAccount, ClaudeUsage, GamesFolder, Session, UsageWindow};

use crate::bridge;
use crate::build::explain_build;
use crate::build_bridge;
use crate::icons::{FolderIcon, KeyIcon, LogOutIcon};
use crate::studio::{explain, use_studio};

#[component]
pub fn SettingsPage(session: Session, on_sign_out: EventHandler<()>) -> Element {
    let studio = use_studio();
    let mut checking = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let mut login_opened = use_signal(|| false);
    let mut usage = use_signal(|| None::<ClaudeUsage>);
    let mut usage_checked = use_signal(|| false);
    let mut settings_signal = studio.settings;
    let settings = settings_signal.read().clone();

    use_future(move || async move {
        usage.set(bridge::claude_usage().await.ok().flatten());
        usage_checked.set(true);
    });

    let mut refresh = move || {
        if checking() {
            return;
        }
        checking.set(true);
        error.set(None);
        spawn(async move {
            usage.set(bridge::claude_usage().await.ok().flatten());
            usage_checked.set(true);
            match bridge::studio_settings().await {
                Ok(updated) => {
                    if updated.account.connected() {
                        login_opened.set(false);
                    }
                    settings_signal.set(Some(updated));
                }
                Err(problem) => error.set(Some(explain(&problem))),
            }
            checking.set(false);
        });
    };

    let login = move |_| {
        error.set(None);
        spawn(async move {
            match bridge::claude_login().await {
                Ok(()) => login_opened.set(true),
                Err(problem) => error.set(Some(explain(&problem))),
            }
        });
    };

    let account = settings.as_ref().map(|settings| settings.account.clone());
    let (status, status_class) = account_status(account.as_ref());
    let model = settings
        .as_ref()
        .map(|settings| settings.model.clone())
        .unwrap_or_default();
    let folder = settings
        .as_ref()
        .and_then(|settings| settings.skills_folder.clone());
    let installed = account.as_ref().is_some_and(|account| account.installed);
    let connected = account.as_ref().is_some_and(ClaudeAccount::connected);
    let version = account
        .as_ref()
        .and_then(|account| account.version.clone())
        .unwrap_or_else(|| "—".to_owned());
    let who = account
        .as_ref()
        .filter(|account| account.signed_in)
        .map(|account| {
            let email = account.email.clone().unwrap_or_default();
            match account.plan.as_deref() {
                Some(plan) => format!("{email} · {}", plan_label(plan)),
                None => email,
            }
        })
        .unwrap_or_else(|| "—".to_owned());

    rsx! {
        section { class: "page",
            header { class: "page-head",
                p { class: "mono-label", "Система · Налаштування" }
                h1 { class: "page-title", "Налаштування" }
                p { class: "page-lead", "Підключення AI-ядра до підписки Claude, навички й акаунт студії." }
            }
            div { class: "settings-stack",
                section { class: "settings-card",
                    header { class: "settings-card-head",
                        KeyIcon {}
                        div {
                            h2 { class: "settings-title", "Підписка Claude" }
                            p { class: "settings-status {status_class}", "{status}" }
                        }
                    }
                    div { class: "settings-key",
                        if installed && !connected {
                            button { class: "primary-button", r#type: "button", onclick: login, "Увійти в Claude" }
                        }
                        button {
                            class: "ghost-button",
                            r#type: "button",
                            disabled: checking(),
                            onclick: move |_| refresh(),
                            if checking() {
                                span { class: "spinner" }
                                "Перевіряємо…"
                            } else {
                                "Перевірити підключення"
                            }
                        }
                    }
                    if login_opened() {
                        p { class: "settings-saved",
                            "Відкрилося вікно входу. Увійди акаунтом claude.ai з підпискою, потім натисни «Перевірити підключення»."
                        }
                    }
                    if !installed && account.is_some() {
                        p { class: "settings-text",
                            "Конвеєр працює через Claude Code — офіційний CLI Claude. Встанови його з claude.com/claude-code, увійди своїм акаунтом і натисни «Перевірити підключення»."
                        }
                    }
                    if let Some(message) = error() {
                        p { class: "alert", role: "alert", "{message}" }
                    }
                    dl { class: "settings-facts",
                        dt { class: "mono-label", "Акаунт" }
                        dd { "{who}" }
                        dt { class: "mono-label", "Модель" }
                        dd { class: "settings-mono", "{model}" }
                        dt { class: "mono-label", "Claude Code" }
                        dd { class: "settings-mono", "{version}" }
                        dt { class: "mono-label", "Оплата" }
                        dd { "Ліміти твоєї підписки Claude — без ключа API" }
                    }
                    if connected {
                        div { class: "settings-usage",
                            p { class: "mono-label", "Використання підписки" }
                            match usage() {
                                Some(current) => rsx! {
                                    UsageMeter { title: "Сесія (5 год)", window: current.session }
                                    UsageMeter { title: "Тиждень (7 днів)", window: current.weekly }
                                },
                                None if usage_checked() => rsx! {
                                    p { class: "settings-text",
                                        "Не вдалося отримати ліміти. Запусти будь-який запит у Claude Code, щоб оновити вхід, і натисни «Перевірити підключення»."
                                    }
                                },
                                None => rsx! {
                                    p { class: "settings-text", "Завантажуємо ліміти…" }
                                },
                            }
                        }
                    }
                }
                GamesFolderCard {}
                section { class: "settings-card",
                    h2 { class: "settings-title", "Навички для Claude" }
                    p { class: "settings-text",
                        "Увімкнені навички з SKILLS ядро передає Claude в кожному запиті, а також синхронізує як SKILL.md:"
                    }
                    p { class: "settings-mono is-path", {folder.unwrap_or_else(|| "—".to_owned())} }
                }
                section { class: "settings-card",
                    h2 { class: "settings-title", "Акаунт студії" }
                    p { class: "settings-text", "{session.name} · {session.email}" }
                    button {
                        class: "ghost-button is-start",
                        r#type: "button",
                        onclick: move |_| on_sign_out.call(()),
                        LogOutIcon {}
                        "Вийти"
                    }
                }
            }
        }
    }
}

#[component]
fn GamesFolderCard() -> Element {
    let mut folder = use_signal(|| None::<GamesFolder>);
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    use_future(move || async move {
        match build_bridge::games_folder().await {
            Ok(current) => folder.set(Some(current)),
            Err(problem) => error.set(Some(explain_build(&problem))),
        }
    });

    let mut apply = move |change: Change| {
        if busy() {
            return;
        }
        busy.set(true);
        error.set(None);
        spawn(async move {
            let outcome = match change {
                Change::Pick => build_bridge::pick_games_folder().await.map(Some),
                Change::Reset => build_bridge::reset_games_folder().await.map(Some),
                Change::Open => build_bridge::open_games_folder().await.map(|()| None),
            };
            match outcome {
                Ok(Some(updated)) => folder.set(Some(updated)),
                Ok(None) => {}
                Err(problem) => error.set(Some(explain_folder(&problem))),
            }
            busy.set(false);
        });
    };

    let current = folder();
    let is_default = current.as_ref().is_none_or(|folder| folder.is_default);
    let status = match &current {
        None => "Перевіряємо…",
        Some(folder) if folder.is_default => "Стандартна папка — у Документах",
        Some(_) => "Своя папка",
    };
    let path = current
        .as_ref()
        .map(|folder| folder.path.clone())
        .unwrap_or_else(|| "—".to_owned());

    rsx! {
        section { class: "settings-card",
            header { class: "settings-card-head",
                FolderIcon {}
                div {
                    h2 { class: "settings-title", "Папка проєктів" }
                    p { class: "settings-status", "{status}" }
                }
            }
            p { class: "settings-text",
                "Тут конвеєр створює кожну нову гру — окрему папку з кодом (source) і збірками (builds). Ігри, які вже згенеровані, лишаються там, де лежать."
            }
            p { class: "settings-mono is-path", "{path}" }
            div { class: "settings-key",
                button {
                    class: "primary-button",
                    r#type: "button",
                    disabled: busy() || current.is_none(),
                    onclick: move |_| apply(Change::Pick),
                    if busy() {
                        span { class: "spinner" }
                    }
                    "Обрати папку…"
                }
                button {
                    class: "ghost-button",
                    r#type: "button",
                    disabled: busy() || current.is_none(),
                    onclick: move |_| apply(Change::Open),
                    "Відкрити"
                }
                if !is_default {
                    button {
                        class: "ghost-button",
                        r#type: "button",
                        disabled: busy(),
                        onclick: move |_| apply(Change::Reset),
                        "Повернути стандартну"
                    }
                }
            }
            if let Some(message) = error() {
                p { class: "alert", role: "alert", "{message}" }
            }
        }
    }
}

#[derive(Clone, Copy)]
enum Change {
    Pick,
    Reset,
    Open,
}

fn explain_folder(problem: &BuildError) -> String {
    match problem {
        BuildError::PathInvalid => "Цю папку не можна використати — обери іншу, повний шлях на диску.".to_owned(),
        other => explain_build(other),
    }
}

const MID_USAGE: u32 = 50;
const HIGH_USAGE: u32 = 75;
const FULL_USAGE: u32 = 95;

#[component]
fn UsageMeter(title: &'static str, window: Option<UsageWindow>) -> Element {
    let Some(window) = window else {
        return rsx! {};
    };
    let level = usage_level(window.percent);
    rsx! {
        div { class: "usage-meter",
            div { class: "usage-head",
                span { "{title}" }
                span { class: "usage-percent", "{window.percent}%" }
            }
            div {
                class: "usage-track",
                role: "progressbar",
                aria_valuemin: "0",
                aria_valuemax: "100",
                aria_valuenow: "{window.percent}",
                div { class: "usage-fill {level}", style: "width: {window.percent}%" }
            }
            if let Some(resets) = window.resets {
                p { class: "usage-reset", "Скинеться {resets}" }
            }
        }
    }
}

fn usage_level(percent: u32) -> &'static str {
    match percent {
        value if value >= FULL_USAGE => "is-full",
        value if value >= HIGH_USAGE => "is-high",
        value if value >= MID_USAGE => "is-mid",
        _ => "",
    }
}

fn account_status(account: Option<&ClaudeAccount>) -> (&'static str, &'static str) {
    match account {
        None => ("Перевіряємо…", ""),
        Some(account) if !account.installed => (
            "Claude Code не знайдено — конвеєр не може звертатися до Claude",
            "is-missing",
        ),
        Some(account) if !account.signed_in => (
            "Claude Code не підключено до підписки — увійди акаунтом claude.ai",
            "is-missing",
        ),
        Some(_) => ("Підключено через підписку Claude", "is-ok"),
    }
}

fn plan_label(plan: &str) -> String {
    match plan {
        "pro" => "Pro".to_owned(),
        "max" => "Max".to_owned(),
        "team" => "Team".to_owned(),
        "enterprise" => "Enterprise".to_owned(),
        other => other.to_owned(),
    }
}
