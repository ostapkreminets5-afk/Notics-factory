use std::rc::Rc;

use dioxus::prelude::*;
use dioxus_logger::tracing;
use noctics_shared::{
    AuthError, Credentials, MAX_NAME_CHARS, MAX_PASSWORD_CHARS, MIN_PASSWORD_CHARS,
    PasswordStrength, Registration, Session, normalize_email,
};

use crate::bridge;
use crate::icons::ArrowRightIcon;
use crate::pipeline::PipelineCanvas;

const SHAKE_ANIMATION: &str = "form-shake";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuthMode {
    SignIn,
    SignUp,
}

#[component]
pub fn AuthScreen(initial_mode: AuthMode, on_signed_in: EventHandler<Session>) -> Element {
    rsx! {
        div { class: "auth",
            Showcase {}
            section { class: "panel",
                AuthForm { initial_mode, on_signed_in }
                footer { class: "panel-foot mono-label",
                    span { "Argon2id · локальне сховище" }
                    span { "build 0.1.0" }
                }
            }
        }
    }
}

#[component]
fn Showcase() -> Element {
    rsx! {
        section { class: "showcase",
            PipelineCanvas {}
            header { class: "showcase-top mono-label",
                span { "AI-ядро Noctics" }
                span { "6 етапів · промпт → збірка" }
            }
            div { class: "showcase-bottom",
                h1 { class: "title",
                    "Ніч твоя. "
                    span { class: "accent", "Створюй." }
                }
                p { class: "lead",
                    "Система створення ігор для тих, хто не спить, коли місто спить. Ядро ШІ проводить ідею через п’ять систем і збирає готову гру."
                }
                p { class: "mono-label", "Windows · macOS · Linux" }
            }
        }
    }
}

#[component]
fn AuthForm(initial_mode: AuthMode, on_signed_in: EventHandler<Session>) -> Element {
    let mut mode = use_signal(|| initial_mode);
    let mut name = use_signal(String::new);
    let mut email = use_signal(String::new);
    let mut password = use_signal(String::new);
    let mut confirmation = use_signal(String::new);
    let mut revealed = use_signal(|| false);
    let mut caps_lock = use_signal(|| None::<&'static str>);
    let mut error = use_signal(|| None::<String>);
    let mut pending = use_signal(|| false);
    let mut shaking = use_signal(|| false);
    let mut email_field = use_signal(|| None::<Rc<MountedData>>);

    let mut fail = move |message: String| {
        error.set(Some(message));
        shaking.set(true);
        pending.set(false);
    };

    let mut switch_to = move |next: AuthMode| {
        if mode() == next {
            return;
        }
        mode.set(next);
        error.set(None);
        password.set(String::new());
        confirmation.set(String::new());
        revealed.set(false);
        if let (AuthMode::SignIn, Some(field)) = (next, email_field()) {
            spawn(async move {
                let _ = field.set_focus(true).await;
            });
        }
    };

    let track_caps_lock = move |field: &'static str| {
        move |event: KeyboardEvent| {
            let active = event
                .data()
                .downcast::<web_sys::KeyboardEvent>()
                .is_some_and(|raw| raw.get_modifier_state("CapsLock"));
            caps_lock.set(active.then_some(field));
        }
    };

    let submit = move |event: FormEvent| {
        event.prevent_default();
        if pending() {
            return;
        }
        error.set(None);
        if mode() == AuthMode::SignIn {
            let credentials = Credentials {
                email: email(),
                password: password(),
            };
            if normalize_email(&credentials.email).is_empty() || credentials.password.is_empty() {
                fail("Введи пошту та пароль.".to_owned());
                return;
            }
            pending.set(true);
            spawn(async move {
                match bridge::sign_in(&credentials).await {
                    Ok(session) => on_signed_in.call(session),
                    Err(problem) => fail(explain(problem)),
                }
            });
        } else {
            let registration = Registration {
                name: name(),
                email: email(),
                password: password(),
            };
            if let Err(problem) = registration.validate() {
                fail(explain(problem));
                return;
            }
            if registration.password != confirmation() {
                fail("Паролі не збігаються.".to_owned());
                return;
            }
            pending.set(true);
            spawn(async move {
                match bridge::sign_up(&registration).await {
                    Ok(session) => on_signed_in.call(session),
                    Err(problem) => fail(explain(problem)),
                }
            });
        }
    };

    let signing_up = mode() == AuthMode::SignUp;
    let (title, subtitle, action, action_busy) = if signing_up {
        (
            "Нова студія",
            "Акаунт зберігається лише на цьому комп’ютері.",
            "Створити акаунт",
            "Створюємо…",
        )
    } else {
        (
            "З поверненням",
            "Увійди, щоб продовжити роботу над проєктами.",
            "Увійти",
            "Перевіряємо…",
        )
    };
    let password_hint = if signing_up {
        format!("Щонайменше {MIN_PASSWORD_CHARS} символів")
    } else {
        "Твій пароль".to_owned()
    };
    let password_kind = if revealed() { "text" } else { "password" };
    let show_strength = signing_up && !password.read().is_empty();
    let (strength_level, strength_label) = strength_badge(PasswordStrength::assess(&password.read()));

    rsx! {
        div {
            class: if shaking() { "form-wrap is-shaking" } else { "form-wrap" },
            "data-mode": if signing_up { "sign-up" } else { "sign-in" },
            onanimationend: move |event| {
                if event.data().animation_name() == SHAKE_ANIMATION {
                    shaking.set(false);
                }
            },
            div { class: "tabs", role: "tablist",
                button {
                    r#type: "button",
                    role: "tab",
                    class: if signing_up { "tab" } else { "tab is-active" },
                    aria_selected: if signing_up { "false" } else { "true" },
                    onclick: move |_| switch_to(AuthMode::SignIn),
                    "Вхід"
                }
                button {
                    r#type: "button",
                    role: "tab",
                    class: if signing_up { "tab is-active" } else { "tab" },
                    aria_selected: if signing_up { "true" } else { "false" },
                    onclick: move |_| switch_to(AuthMode::SignUp),
                    "Реєстрація"
                }
            }
            header { class: "form-head",
                h2 { "{title}" }
                p { "{subtitle}" }
            }
            form { class: "form", novalidate: true, onsubmit: submit,
                if signing_up {
                    div { class: "field is-entering",
                        div { class: "field-head",
                            label { class: "mono-label", r#for: "auth-name", "Ім’я" }
                        }
                        input {
                            class: "input",
                            id: "auth-name",
                            r#type: "text",
                            placeholder: "Як тебе називати",
                            autocomplete: "nickname",
                            maxlength: "{MAX_NAME_CHARS}",
                            value: "{name}",
                            oninput: move |event| name.set(event.value()),
                            onmounted: move |event| async move {
                                let _ = event.data().set_focus(true).await;
                            },
                        }
                    }
                }
                div { class: "field",
                    div { class: "field-head",
                        label { class: "mono-label", r#for: "auth-email", "Пошта" }
                    }
                    input {
                        class: "input",
                        id: "auth-email",
                        r#type: "email",
                        placeholder: "you@studio.ua",
                        autocomplete: "username",
                        spellcheck: "false",
                        value: "{email}",
                        oninput: move |event| email.set(event.value()),
                        onmounted: move |event| async move {
                            let field = event.data();
                            email_field.set(Some(field.clone()));
                            if initial_mode == AuthMode::SignIn {
                                let _ = field.set_focus(true).await;
                            }
                        },
                    }
                }
                div { class: "field",
                    div { class: "field-head",
                        label { class: "mono-label", r#for: "auth-password", "Пароль" }
                        if caps_lock() == Some("password") {
                            span { class: "mono-label caps", "Caps Lock" }
                        } else if show_strength {
                            div { class: "strength", "data-level": "{strength_level}",
                                span { class: "strength-bars",
                                    i {}
                                    i {}
                                    i {}
                                    i {}
                                }
                                em { class: "mono-label", "{strength_label}" }
                            }
                        }
                    }
                    div { class: "control",
                        input {
                            class: "input",
                            id: "auth-password",
                            r#type: password_kind,
                            placeholder: "{password_hint}",
                            autocomplete: if signing_up { "new-password" } else { "current-password" },
                            maxlength: "{MAX_PASSWORD_CHARS}",
                            value: "{password}",
                            oninput: move |event| password.set(event.value()),
                            onkeydown: track_caps_lock("password"),
                            onkeyup: track_caps_lock("password"),
                            onblur: move |_| caps_lock.set(None),
                        }
                        button {
                            class: "reveal mono-label",
                            r#type: "button",
                            aria_pressed: if revealed() { "true" } else { "false" },
                            onmousedown: move |event| event.prevent_default(),
                            onclick: move |_| revealed.toggle(),
                            if revealed() {
                                "Сховати"
                            } else {
                                "Показати"
                            }
                        }
                    }
                }
                if signing_up {
                    div { class: "field is-entering",
                        div { class: "field-head",
                            label { class: "mono-label", r#for: "auth-confirmation", "Повтор пароля" }
                            if caps_lock() == Some("confirmation") {
                                span { class: "mono-label caps", "Caps Lock" }
                            }
                        }
                        input {
                            class: "input",
                            id: "auth-confirmation",
                            r#type: password_kind,
                            placeholder: "Ще раз той самий",
                            autocomplete: "new-password",
                            maxlength: "{MAX_PASSWORD_CHARS}",
                            value: "{confirmation}",
                            oninput: move |event| confirmation.set(event.value()),
                            onkeydown: track_caps_lock("confirmation"),
                            onkeyup: track_caps_lock("confirmation"),
                            onblur: move |_| caps_lock.set(None),
                        }
                    }
                }
                if let Some(message) = error() {
                    p { class: "alert", role: "alert", "{message}" }
                }
                button { class: "submit", r#type: "submit", disabled: pending(),
                    span {
                        if pending() {
                            "{action_busy}"
                        } else {
                            "{action}"
                        }
                    }
                    if pending() {
                        span { class: "spinner" }
                    } else {
                        ArrowRightIcon {}
                    }
                }
            }
        }
    }
}

fn strength_badge(strength: PasswordStrength) -> (u8, &'static str) {
    match strength {
        PasswordStrength::Weak => (1, "Слабкий"),
        PasswordStrength::Fair => (2, "Так собі"),
        PasswordStrength::Good => (3, "Добрий"),
        PasswordStrength::Strong => (4, "Надійний"),
    }
}

fn explain(problem: AuthError) -> String {
    match problem {
        AuthError::InvalidCredentials => "Невірна пошта або пароль.".to_owned(),
        AuthError::EmailTaken => "Ця пошта вже зареєстрована — увійди.".to_owned(),
        AuthError::InvalidEmail => "Схоже, у пошті помилка.".to_owned(),
        AuthError::PasswordTooShort => {
            format!("Пароль має містити щонайменше {MIN_PASSWORD_CHARS} символів.")
        }
        AuthError::PasswordTooLong => {
            format!("Пароль задовгий — максимум {MAX_PASSWORD_CHARS} символів.")
        }
        AuthError::NameMissing => "Вкажи ім’я — так тебе бачитиме студія.".to_owned(),
        AuthError::NameTooLong => format!("Ім’я задовге — максимум {MAX_NAME_CHARS} символів."),
        AuthError::Storage(detail) => {
            tracing::error!("account storage failed: {detail}");
            "Не вдалося звернутися до сховища. Спробуй ще раз.".to_owned()
        }
    }
}
