use dioxus::prelude::*;

#[component]
fn Glyph(children: Element) -> Element {
    rsx! {
        svg {
            class: "icon",
            view_box: "0 0 24 24",
            fill: "none",
            stroke: "currentColor",
            stroke_width: "1.6",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            "aria-hidden": "true",
            {children}
        }
    }
}

#[component]
fn WindowGlyph(children: Element) -> Element {
    rsx! {
        svg { view_box: "0 0 10 10", "aria-hidden": "true", {children} }
    }
}

#[component]
pub fn LogoMark() -> Element {
    rsx! {
        svg { class: "logo-mark", view_box: "0 0 24 24", "aria-hidden": "true",
            rect { x: "1", y: "1", width: "22", height: "22", rx: "6", fill: "#3b7bff" }
            path { d: "M6.5 17.5V6.5h3l5 6v-6h3v11h-3l-5-6v6z", fill: "#ffffff" }
        }
    }
}

#[component]
pub fn ArrowRightIcon() -> Element {
    rsx! {
        Glyph {
            path { d: "M5 12h14" }
            path { d: "m12 5 7 7-7 7" }
        }
    }
}

#[component]
pub fn LogOutIcon() -> Element {
    rsx! {
        Glyph {
            path { d: "M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4" }
            path { d: "m16 17 5-5-5-5" }
            path { d: "M21 12H9" }
        }
    }
}

#[component]
pub fn CpuIcon() -> Element {
    rsx! {
        Glyph {
            rect { x: "4", y: "4", width: "16", height: "16", rx: "2" }
            rect { x: "9", y: "9", width: "6", height: "6", rx: "1" }
            path { d: "M15 2v2M15 20v2M2 15h2M2 9h2M20 15h2M20 9h2M9 2v2M9 20v2" }
        }
    }
}

#[component]
pub fn StarIcon() -> Element {
    rsx! {
        Glyph {
            path { d: "M11.525 2.295a.53.53 0 0 1 .95 0l2.31 4.679a2.123 2.123 0 0 0 1.595 1.16l5.166.756a.53.53 0 0 1 .294.904l-3.736 3.638a2.123 2.123 0 0 0-.611 1.878l.882 5.14a.53.53 0 0 1-.771.56l-4.618-2.428a2.122 2.122 0 0 0-1.973 0L6.396 21.01a.53.53 0 0 1-.77-.56l.881-5.139a2.122 2.122 0 0 0-.611-1.879L2.16 9.795a.53.53 0 0 1 .294-.906l5.165-.755a2.122 2.122 0 0 0 1.597-1.16z" }
        }
    }
}

#[component]
pub fn RouteIcon() -> Element {
    rsx! {
        Glyph {
            circle { cx: "6", cy: "19", r: "3" }
            path { d: "M9 19h8.5a3.5 3.5 0 0 0 0-7h-11a3.5 3.5 0 0 1 0-7H15" }
            circle { cx: "18", cy: "5", r: "3" }
        }
    }
}

#[component]
pub fn PanelsIcon() -> Element {
    rsx! {
        Glyph {
            rect { x: "3", y: "3", width: "18", height: "18", rx: "2" }
            path { d: "M3 9h18M9 21V9" }
        }
    }
}

#[component]
pub fn SidebarIcon() -> Element {
    rsx! {
        Glyph {
            rect { x: "3", y: "3", width: "18", height: "18", rx: "2" }
            path { d: "M9 3v18" }
            path { class: "sidebar-chevron", d: "m16 15-3-3 3-3" }
        }
    }
}

#[component]
pub fn FolderIcon() -> Element {
    rsx! {
        Glyph {
            path { d: "M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z" }
        }
    }
}

#[component]
pub fn GridIcon() -> Element {
    rsx! {
        Glyph {
            rect { x: "3", y: "3", width: "7", height: "7", rx: "1" }
            rect { x: "14", y: "3", width: "7", height: "7", rx: "1" }
            rect { x: "14", y: "14", width: "7", height: "7", rx: "1" }
            rect { x: "3", y: "14", width: "7", height: "7", rx: "1" }
        }
    }
}

#[component]
pub fn BoxIcon() -> Element {
    rsx! {
        Glyph {
            path { d: "M21 8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16Z" }
            path { d: "m3.3 7 8.7 5 8.7-5M12 22V12" }
        }
    }
}

#[component]
pub fn PackageIcon() -> Element {
    rsx! {
        Glyph {
            path { d: "M11 21.73a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73z" }
            path { d: "M12 22V12m-8.7-5 7.703 4.734a2 2 0 0 0 1.994 0L20.7 7M7.5 4.27l9 5.15" }
        }
    }
}

#[component]
pub fn ListIcon() -> Element {
    rsx! {
        Glyph {
            path { d: "M3 6h.01M3 12h.01M3 18h.01M8 6h13M8 12h13M8 18h13" }
        }
    }
}

#[component]
pub fn SettingsIcon() -> Element {
    rsx! {
        Glyph {
            path { d: "M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" }
            circle { cx: "12", cy: "12", r: "3" }
        }
    }
}

#[component]
pub fn CloseIcon() -> Element {
    rsx! {
        Glyph { path { d: "M18 6 6 18M6 6l12 12" } }
    }
}

#[component]
pub fn PencilIcon() -> Element {
    rsx! {
        Glyph {
            path { d: "M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z" }
            path { d: "m15 5 4 4" }
        }
    }
}

#[component]
pub fn KeyIcon() -> Element {
    rsx! {
        Glyph {
            path { d: "m15.5 7.5 2.3 2.3a1 1 0 0 0 1.4 0l2.1-2.1a1 1 0 0 0 0-1.4L19 4" }
            path { d: "m21 2-9.6 9.6" }
            circle { cx: "7.5", cy: "15.5", r: "5.5" }
        }
    }
}

#[component]
pub fn ChevronsIcon() -> Element {
    rsx! {
        Glyph { path { d: "m7 15 5 5 5-5M7 9l5-5 5 5" } }
    }
}

#[component]
pub fn CodeIcon() -> Element {
    rsx! {
        Glyph { path { d: "m16 18 6-6-6-6M8 6l-6 6 6 6" } }
    }
}

#[component]
pub fn ChevronDownIcon() -> Element {
    rsx! {
        Glyph { path { d: "m6 9 6 6 6-6" } }
    }
}

#[component]
pub fn ChevronRightIcon() -> Element {
    rsx! {
        Glyph { path { d: "m9 6 6 6-6 6" } }
    }
}

#[component]
pub fn ChevronLeftIcon() -> Element {
    rsx! {
        Glyph { path { d: "m15 6-6 6 6 6" } }
    }
}

#[component]
pub fn ExpandIcon() -> Element {
    rsx! {
        Glyph {
            path { d: "M15 3h6v6" }
            path { d: "M9 21H3v-6" }
            path { d: "m21 3-7 7" }
            path { d: "m3 21 7-7" }
        }
    }
}

#[component]
pub fn SearchIcon() -> Element {
    rsx! {
        Glyph {
            circle { cx: "11", cy: "11", r: "7" }
            path { d: "m20 20-3.5-3.5" }
        }
    }
}

#[component]
pub fn PlusIcon() -> Element {
    rsx! {
        Glyph { path { d: "M12 5v14M5 12h14" } }
    }
}

#[component]
pub fn CheckIcon() -> Element {
    rsx! {
        Glyph { path { d: "M20 6 9 17l-5-5" } }
    }
}

#[component]
pub fn ExternalIcon() -> Element {
    rsx! {
        Glyph {
            path { d: "M15 3h6v6M10 14 21 3" }
            path { d: "M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" }
        }
    }
}

#[component]
pub fn RefreshIcon() -> Element {
    rsx! {
        Glyph {
            path { d: "M3 12a9 9 0 0 1 15.3-6.4L21 8" }
            path { d: "M21 3v5h-5" }
            path { d: "M21 12a9 9 0 0 1-15.3 6.4L3 16" }
            path { d: "M3 21v-5h5" }
        }
    }
}

#[component]
pub fn TrashIcon() -> Element {
    rsx! {
        Glyph {
            path { d: "M3 6h18M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2" }
        }
    }
}

#[component]
pub fn MotionIcon() -> Element {
    rsx! {
        Glyph {
            path { d: "M22 12h-4l-3 9L9 3l-3 9H2" }
        }
    }
}

#[component]
pub fn MinimizeGlyph() -> Element {
    rsx! {
        WindowGlyph { path { d: "M0 5h10" } }
    }
}

#[component]
pub fn MaximizeGlyph() -> Element {
    rsx! {
        WindowGlyph {
            rect { x: ".5", y: ".5", width: "9", height: "9", rx: "1.5" }
        }
    }
}

#[component]
pub fn RestoreGlyph() -> Element {
    rsx! {
        WindowGlyph {
            rect { x: ".5", y: "2.5", width: "7", height: "7", rx: "1.2" }
            path { d: "M2.5 2.5V1.8A1.3 1.3 0 0 1 3.8.5h4.4a1.3 1.3 0 0 1 1.3 1.3v4.4a1.3 1.3 0 0 1-1.3 1.3h-.7" }
        }
    }
}

#[component]
pub fn CloseGlyph() -> Element {
    rsx! {
        WindowGlyph { path { d: "M.5.5l9 9M9.5.5l-9 9" } }
    }
}

#[component]
pub fn NodesIcon() -> Element {
    rsx! {
        Glyph {
            rect { x: "2", y: "3", width: "7", height: "6", rx: "1.5" }
            rect { x: "15", y: "15", width: "7", height: "6", rx: "1.5" }
            rect { x: "15", y: "3", width: "7", height: "6", rx: "1.5" }
            path { d: "M9 6h6" }
            path { d: "M9 6h1.5a2 2 0 0 1 2 2v8a2 2 0 0 0 2 2H15" }
        }
    }
}

#[component]
pub fn LayoutIcon() -> Element {
    rsx! {
        Glyph {
            rect { x: "3", y: "3", width: "18", height: "18", rx: "2" }
            path { d: "M9 3v18" }
            path { d: "M13 8h4" }
            path { d: "M13 12h4" }
        }
    }
}
