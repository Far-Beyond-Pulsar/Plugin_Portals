//! Portals (`pulsar.portals`): the editor crate -- the portal diagnostics
//! overlay of the level editor's viewport ([`level_editor`]): it pairs
//! portals through the component crate and publishes what the overlay
//! draws as the overlay's view payloads. It registers nothing itself: the
//! plugin's root crate (`portals_plugin`, with its `editor` feature) submits
//! the overlay through the level editor's extension points. The inspector
//! edits `PortalComponent` through its reflected properties.

pub mod level_editor;
