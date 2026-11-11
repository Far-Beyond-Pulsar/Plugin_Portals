//! Portals (`pulsar.portals`): the plugin's root crate, the one place the
//! plugin registers everything it contributes.
//!
//! The plugin's other crates hold the code: the feature crates
//! (`helio-pass-portal-cull`: pairing views, recursion chains and culling,
//! with the portal settings; `helio-pass-portal-instances`: the portal
//! draws, the mask and the editor overlay), `portals_component` (the SceneDB
//! component, pairing and projection) and `portals_editor` (the viewport's
//! portal diagnostics overlay). None of them registers a contribution; this
//! crate imports them and registers their registrations, so this file and the
//! two modules below are the whole list of what the plugin brings:
//!
//! - [`runtime`]: what a game links. The component class, the space
//!   producer (pairing and projection each frame), the Helio features and
//!   the project's portal settings.
//! - `editor` (with the `editor` feature): what the editor adds. The
//!   viewport's portal links overlay.
//!
//! Every registration names this crate as its origin
//! (`pulsar_plugins::origin!()`), so the plugin gate skips all of them while
//! a project disables the plugin, and the plugin catalog lists them under
//! `pulsar.portals`. Other plugins never depend on this crate; hosts reach
//! it through the installed set or a generated game, a game without the
//! `editor` feature.

// The plugin's descriptor (`pulsar-plugin.toml`): linking this crate installs
// the plugin; a project enables it or not (`pulsar_plugins`).
pulsar_plugins::installed!("pulsar-plugin.toml", build);

pub use portals_component;
#[cfg(feature = "editor")]
pub use portals_editor;

pub mod runtime;

#[cfg(feature = "editor")]
pub mod editor;

/// Everything the plugin registers (runtime always, editor with the `editor`
/// feature), through the builder the kernel hands the plugin's root.
pub fn build(p: &mut pulsar_plugins::PluginBuilder) {
    runtime::build(p);
    #[cfg(feature = "editor")]
    editor::build(p);
}
