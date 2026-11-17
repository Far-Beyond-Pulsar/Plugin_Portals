# Plugin_Portals

Openings that show another place: their component, pairing and projection, settings, and the renderer feature through which every drawer draws what each opening shows: a portal is a map between coordinate spaces, and a route through openings one composed map.

`pulsar.portals` ("Portals") is a plugin of
[Pulsar-Native](https://github.com/Far-Beyond-Pulsar/Pulsar-Native), the engine and editor that renders with
[Helio](https://github.com/Far-Beyond-Pulsar/Helio). Pulsar-Native vendors this repository as a git submodule
at `plugins/vendor/portals`.

## What it owns

Every object kind is a plugin. A plugin owns its SceneDB components (their
data and scriptable methods; the editor calls the same methods scripts do),
the contracts other kinds read (its `api` crate: graph keys, row ABIs, WGSL
snippets), the Helio feature that renders it (passes, scene derivations,
settings), its file formats and its editor UI. Helio knows no object kind,
and other kinds meet this one only through frame contracts, extension points,
draw phases and api crates.

| Crate | Path | What |
|---|---|---|
| `portals_component` | `component/` | Portals (pulsar.portals): the portal component, pairing and projection, and the plugin's Helio feature set and its settings |
| `portals_plugin` | `.` | Portals (`pulsar.portals`): the plugin's root crate, the one place it registers everything it contributes (runtime registrations always, the editor's with `editor`) |
| `portals_editor` | `editor/` | Portals (pulsar.portals): the editor crate -- the viewport's portal diagnostics overlay, over the plugin's component crate |
| `portals-examples` | `examples/` | The portals plugin's standalone hosts on Helio: portal demos |
| `helio-portal-api` | `api/` | The portals' public contract: the frame's chains (CPU set, GPU rows), the aperture mask, the draw orders around them, and the WGSL a drawer includes to draw through a chain |
| `helio-pass-portal-cull` | `feature/cull/` | The portals' CPU projection of authored portals, the chains it publishes, and the plugin's settings |
| `helio-pass-portal-instances` | `feature/instances/` | The portals' passes: the chains every drawer draws its content through, the aperture mask, and the editor overlay on the openings |

The root crate (`portals_plugin`, this directory's package) is the one place
the plugin registers what it contributes: its component classes, every
registration that names an origin (`pulsar_plugins::origin!()`) and the
descriptor. The other crates hold the code and register nothing but their
types' own attachments, so `src/runtime.rs` (and `src/editor.rs`, with
the `editor` feature) lists everything the plugin brings. The plugin's
integration tests (`tests/`) link the root, as a game does.

- **Runtime module** (what a game built for a project links): `portals_plugin`.
- **Editor module** (what the editor links in addition): `portals_plugin` with its
  `editor` feature (`portals_editor`).
- **Requires**: nothing. A project enables or disables the plugin in its
  `Pulsar.toml` (`[plugins]`); disabled, the kind is absent and the frame
  draws without it.
- **Reads other kinds through**: none. Other kinds read the portals through `helio-portal-api`: each drawer emits its own (content, chain) occurrences and draws them with its own program (`//!use portal_chain`); lighting and decals map a chain's pixels back through `lighting::VIEW_REMAPS`.
- **Its standalone hosts also compose**: `helio-pass-gi` (gi); `helio-light-api, helio-light-host` (lights); `helio-mats` (materials); `helio-mesh-api, helio-mesh-host, helio-pass-forward-lit` (meshes); `helio-pass-sky` (sky) (demos and GPU tests
  draw a whole scene; the plugin itself does not depend on these).

## Building and testing

Like [Plugin_Physics](https://github.com/Far-Beyond-Pulsar/Plugin_Physics), this plugin's crates are members
of Pulsar-Native's workspace: their dependencies are `workspace = true`, and
Helio comes from Pulsar-Native's `crates/renderer/helio` submodule. It builds
only inside a Pulsar-Native checkout:

```bash
git clone --recursive https://github.com/Far-Beyond-Pulsar/Pulsar-Native
cd Pulsar-Native
cargo check -p portals_plugin -p portals_component -p helio-portal-api -p portals_editor --features portals_plugin/editor -p portals-examples -p helio-pass-portal-cull -p helio-pass-portal-instances --tests
cargo test -p portals_plugin -p portals_component -p helio-portal-api -p helio-pass-portal-cull -p helio-pass-portal-instances
```

Suites that render real frames need a GPU; CI leaves them out by name
(Pulsar-Native's `.config/nextest.toml`, `ci` profile). This repository's CI
checks the plugin inside a Pulsar-Native checkout: rustfmt, clippy and the
`ci` profile's tests.

A change that spans this plugin and Pulsar-Native (or Helio) merges
dependency-first: merge here, re-pin `plugins/vendor/portals` in Pulsar-Native to
this repository's `main`, then merge the Pulsar-Native PR (Pulsar-Native's
`.agents/CRATES.md`, "Submodule pins").

## Architecture

- [Every object kind is a plugin](https://github.com/Far-Beyond-Pulsar/Helio/blob/main/docs/architecture/plugin-migration.md): the
  rule, what Helio is, the plugin table and the "As built" notes of each
  phase.
- [The layered rewrite](https://github.com/Far-Beyond-Pulsar/Helio/blob/main/docs/architecture/layered-rewrite.md): layers, buffer
  contracts, extension points and why the renderer has this shape.
- [The `helio-architecture` skill](https://github.com/Far-Beyond-Pulsar/Helio/blob/main/.agents/skills/helio-architecture/SKILL.md): required reading for any change
  here (see `AGENTS.md`).
- Pulsar-Native's `.agents/PLUGIN_SYSTEM.md` and `.agents/CRATES.md`: plugin
  descriptors, the installed set and the submodule pins.

History before 2026-11-02 (when the plugin moved into Pulsar-Native) is in
Helio's repository; this repository's history starts with that move
(`git subtree split` of `plugins/vendor/portals`).

## License

MIT, as the rest of Pulsar (`LICENSE.md`).
