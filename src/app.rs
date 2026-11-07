use dioxus::prelude::*;
use noctics_shared::Session;

use crate::auth::{AuthMode, AuthScreen};
use crate::titlebar::Titlebar;
use crate::ui_zoom::UiZoom;
use crate::workspace::Workspace;
use crate::{bridge, fonts};

static BASE_CSS: Asset = asset!("/assets/styles/base.css");
static TITLEBAR_CSS: Asset = asset!("/assets/styles/titlebar.css");
static AUTH_CSS: Asset = asset!("/assets/styles/auth.css");
static HOME_CSS: Asset = asset!("/assets/styles/home.css");
static WORKSPACE_CSS: Asset = asset!("/assets/styles/workspace.css");
static SKILLS_CSS: Asset = asset!("/assets/styles/skills.css");
static STUDIO_CSS: Asset = asset!("/assets/styles/studio.css");
static TOOLS_CSS: Asset = asset!("/assets/styles/tools.css");
static JOURNAL_CSS: Asset = asset!("/assets/styles/journal.css");
static BUILD_CSS: Asset = asset!("/assets/styles/build.css");
static ARTIFACTS_CSS: Asset = asset!("/assets/styles/artifacts.css");
static COMFY_CSS: Asset = asset!("/assets/styles/comfy.css");
static NODES_CSS: Asset = asset!("/assets/styles/nodes.css");
static ASSET_HUNT_CSS: Asset = asset!("/assets/styles/asset_hunt.css");
static MIXAMO_CSS: Asset = asset!("/assets/styles/mixamo.css");
static UI_ZOOM_CSS: Asset = asset!("/assets/styles/ui_zoom.css");

#[derive(Clone, PartialEq)]
enum Screen {
    Starting,
    SignedOut(AuthMode),
    SignedIn(Session),
}

#[component]
pub fn App() -> Element {
    let mut screen = use_signal(|| Screen::Starting);
    let font_faces = use_hook(fonts::font_faces);

    use_future(move || async move {
        let resumed = match bridge::current_session().await {
            Ok(Some(session)) => Screen::SignedIn(session),
            _ => match bridge::has_accounts().await {
                Ok(false) => Screen::SignedOut(AuthMode::SignUp),
                _ => Screen::SignedOut(AuthMode::SignIn),
            },
        };
        screen.set(resumed);
    });

    let in_workspace = matches!(screen(), Screen::SignedIn(_));
    let stage = match screen() {
        Screen::Starting => rsx! {},
        Screen::SignedOut(mode) => rsx! {
            AuthScreen {
                initial_mode: mode,
                on_signed_in: move |session| screen.set(Screen::SignedIn(session)),
            }
        },
        Screen::SignedIn(session) => rsx! {
            Workspace {
                session,
                on_sign_out: move |_| async move {
                    let _ = bridge::sign_out().await;
                    screen.set(Screen::SignedOut(AuthMode::SignIn));
                },
            }
        },
    };

    rsx! {
        document::Style { "{font_faces}" }
        document::Stylesheet { href: BASE_CSS }
        document::Stylesheet { href: TITLEBAR_CSS }
        document::Stylesheet { href: AUTH_CSS }
        document::Stylesheet { href: HOME_CSS }
        document::Stylesheet { href: WORKSPACE_CSS }
        document::Stylesheet { href: SKILLS_CSS }
        document::Stylesheet { href: STUDIO_CSS }
        document::Stylesheet { href: TOOLS_CSS }
        document::Stylesheet { href: JOURNAL_CSS }
        document::Stylesheet { href: BUILD_CSS }
        document::Stylesheet { href: ARTIFACTS_CSS }
        document::Stylesheet { href: COMFY_CSS }
        document::Stylesheet { href: NODES_CSS }
        document::Stylesheet { href: ASSET_HUNT_CSS }
        document::Stylesheet { href: MIXAMO_CSS }
        document::Stylesheet { href: UI_ZOOM_CSS }
        div { class: if in_workspace { "shell is-workspace" } else { "shell" },
            Titlebar { overlay: in_workspace }
            main { class: "stage", {stage} }
        }
        UiZoom {}
    }
}
