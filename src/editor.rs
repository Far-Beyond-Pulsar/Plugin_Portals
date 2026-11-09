//! What the portals plugin registers for the editor (the `editor` feature):
//! its viewport overlay.

use portals_editor::level_editor;
use ui_level_editor::extend::viewport::DebugOverlay;

// ── Viewport ────────────────────────────────────────────────────────────────

// The portal links overlay: the link list, a badge over each portal and the
// ghost geometry of a selected direction; the portal debug view shows it.
inventory::submit! {
    DebugOverlay {
        origin: pulsar_plugins::origin!(),
        id: "portal_links",
        menu_label: "Portal links",
        debug_mode: Some(portals_component::PORTAL_DEBUG_VIEW_MODE),
        panel: level_editor::overlay_panel,
        perspective: level_editor::overlay_perspective,
        hidden: level_editor::overlay_hidden,
    }
}
