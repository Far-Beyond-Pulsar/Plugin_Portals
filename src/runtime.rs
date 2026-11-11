//! What the portals plugin registers for a game (and the editor): every
//! runtime contribution, grouped by the engine seam it plugs into.

use engine_backend::scene::SpaceProducerRegistration;
use engine_backend::scene::{ClassSpec, ComponentsBuilder};
use engine_backend::subsystems::render::RenderFeatureRegistration;
use portals_component::{PortalComponent, PortalSync};
use pulsar_settings::project::rendering::SettingsGroupRegistration;

/// Everything the portals plugin registers for a game.
pub(crate) fn build(p: &mut pulsar_plugins::PluginBuilder) {
    // ── Component classes ───────────────────────────────────────────────────

    p.components()
        .class::<PortalComponent>(ClassSpec::default());

    // ── Scene ───────────────────────────────────────────────────────────────

    // Each frame, every renderer pairs the authored portals and expands them
    // into views and recursion chains with slots in the frame's space table.
    p.add(SpaceProducerRegistration {
        origin: p.origin(),
        name: "portals",
        new: || Box::new(PortalSync::default()),
    });

    // ── Rendering ───────────────────────────────────────────────────────────

    // Portal culling and the portal draws (a G-buffer drawer, the mask and
    // the editor overlay), configured from the project's portal settings.
    p.add(RenderFeatureRegistration {
        origin: p.origin(),
        features: portals_component::render_features,
        configure: portals_component::configure_renderer,
    });

    // ── Settings ────────────────────────────────────────────────────────────

    // The `project.portals` settings group.
    p.add(SettingsGroupRegistration {
        origin: p.origin(),
        group: &helio_pass_portal_cull::settings::PORTALS,
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn registers_its_features_settings_and_space_producer() {
        let features =
            engine_backend::subsystems::render::helio_renderer::render_features::plugin_features();
        for id in ["helio.portal_cull", "helio.portals"] {
            assert!(
                features.ids().contains(&id),
                "{id}: features {:?}",
                features.ids()
            );
        }
        assert!(pulsar_settings::project::rendering::groups()
            .iter()
            .any(|group| std::ptr::eq(*group, &helio_pass_portal_cull::settings::PORTALS)));
        assert!(
            pulsar_plugins::registered::<engine_backend::scene::SpaceProducerRegistration>()
                .any(|registration| registration.name == "portals")
        );
    }

    /// Every registration of the plugin names this root, so it resolves to
    /// the plugin's id.
    #[test]
    fn every_registration_resolves_to_the_plugin() {
        let ours: Vec<_> = pulsar_plugins::contributions()
            .into_iter()
            .filter(|summary| summary.origin == pulsar_plugins::origin!())
            .collect();
        assert!(ours
            .iter()
            .any(|summary| summary.category == "component.class"));
        for summary in &ours {
            assert_eq!(
                pulsar_plugins::plugin_of(summary.origin).map(|plugin| plugin.id.as_str()),
                Some("pulsar.portals"),
                "{summary:?}"
            );
        }
    }
}
