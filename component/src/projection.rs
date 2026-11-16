//! Portals: pairing authored portals and feeding the portal passes.
//!
//! The only authored thing is [`PortalComponent`](crate::PortalComponent): an
//! opening, its size, and a **Link ID**. This module turns the authored
//! portals of a world into what the portal passes draw, each frame:
//!
//! 1. [`pair_portals`] groups enabled, visible portals by Link ID. Exactly two
//!    with the same non-zero ID are partners; one alone or three or more are
//!    reported as problems and drawn as nothing.
//! 2. Each portal's pose is its object's world pose (rotation and translation;
//!    scale sizes the opening), relative to the frame's world origin like every
//!    other row the renderer reads.
//! 3. The feature's `SubLevelResolver` + `PortalProjectionBridge` turn the
//!    pairs into the frame's openings, each with the view map that places
//!    what it shows beyond it, and every route through them (a portal seen
//!    through portals) up to the settings' depth and count
//!    (`helio_pass_portal_cull::settings::PORTALS`).
//! 4. They go to the frame as `helio_portal_api::PORTAL_CHAIN_SET`: the
//!    portals' chains pass keeps the routes the camera sees and every drawer
//!    draws its content through them into the frame's own G-buffer. Nothing
//!    takes a slot in the frame's coordinate-space table: a route carries
//!    its composed map.
//!
//! [`PortalSync`] does it for one renderer: the engine makes one per renderer
//! (a registered space producer, `engine_backend::scene::SpaceProducer`).
//!
//! Portals inside a level instance need nothing extra: placed content is real
//! objects in the world, so a portal in a displayed level pairs with a portal
//! outside it exactly like any other.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use engine_backend::scene::{SpaceProducer, SpaceTable};
use glam::{DMat4, DQuat, DVec3, Mat4};
use helio_pass_portal_cull::components::PortalRow;
use helio_pass_portal_cull::{
    PortalProjectionBridge, PortalProjectionConfig, PortalSettings, SubLevelContents,
    SubLevelResolver, chain_set,
};
use helio_portal_api::{PORTAL_CHAIN_SET, PortalChainSet};
use pulsar_scene_model::Visibility;
use pulsar_scene_model::attachments::enabled_components;
use pulsar_scene_model::hierarchy_transform::world_matrix_of;
use pulsar_scenedb::{Entity, World};

use crate::PortalComponent as AuthoredPortal;

/// One enabled, visible authored portal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Portal {
    /// The component instance (unique even if an object holds several).
    pub instance: Entity,
    pub owner: Entity,
    pub link_id: i32,
    pub width: f32,
    pub height: f32,
}

/// Why a Link ID shows nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PortalProblem {
    /// One enabled portal has this ID and no partner.
    Alone { link_id: i32 },
    /// More than two portals share this ID.
    Crowded { link_id: i32, count: usize },
}

impl std::fmt::Display for PortalProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Alone { link_id } => {
                write!(
                    f,
                    "portal Link ID {link_id} has no partner (needs exactly two)"
                )
            }
            Self::Crowded { link_id, count } => write!(
                f,
                "portal Link ID {link_id} is shared by {count} portals (needs exactly two)"
            ),
        }
    }
}

/// The partners found in a world, and the Link IDs that did not pair.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pairing {
    pub pairs: Vec<(Portal, Portal)>,
    pub problems: Vec<PortalProblem>,
}

/// Every enabled, visible authored portal.
pub fn authored_portals(world: &World) -> Vec<Portal> {
    let mut portals: Vec<Portal> = enabled_components::<AuthoredPortal>(world)
        .filter(|(_, owner, portal)| {
            portal.enabled
                && portal.link_id != 0
                && world
                    .get::<Visibility>(*owner)
                    .is_none_or(|visibility| visibility.visible)
        })
        .map(|(instance, owner, portal)| Portal {
            instance,
            owner,
            link_id: portal.link_id,
            width: portal.width,
            height: portal.height,
        })
        .collect();
    portals.sort_by_key(|portal| portal.instance.bits());
    portals
}

/// Group portals by Link ID: exactly two make a pair.
pub fn pair_portals(world: &World) -> Pairing {
    let mut by_id: BTreeMap<i32, Vec<Portal>> = BTreeMap::new();
    for portal in authored_portals(world) {
        by_id.entry(portal.link_id).or_default().push(portal);
    }
    let mut pairing = Pairing::default();
    for (link_id, group) in by_id {
        match group.as_slice() {
            [a, b] => pairing.pairs.push((*a, *b)),
            [_] => pairing.problems.push(PortalProblem::Alone { link_id }),
            many => pairing.problems.push(PortalProblem::Crowded {
                link_id,
                count: many.len(),
            }),
        }
    }
    pairing
}

/// A portal's pose and opening: the object's world rotation and translation
/// (relative to `origin`), and half the opening's extent scaled by the object's
/// world scale along its own X and Y.
fn pose_of(world: &World, portal: &Portal, origin: DVec3) -> Option<(Mat4, [f32; 2])> {
    let matrix = world_matrix_of(world, portal.owner)?;
    let (scale, rotation, translation) = matrix.to_scale_rotation_translation();
    if !(scale.x.is_finite() && scale.y.is_finite()) {
        return None;
    }
    let pose = DMat4::from_rotation_translation(
        if rotation.is_finite() {
            rotation
        } else {
            DQuat::IDENTITY
        },
        translation - origin,
    );
    let half = [
        (f64::from(portal.width) * scale.x.abs() * 0.5) as f32,
        (f64::from(portal.height) * scale.y.abs() * 0.5) as f32,
    ];
    Some((pose.as_mat4(), half))
}

/// The resolver for `pairing`: one sublevel (the default level, in which all
/// portals of the world live), each portal naming its partner.
pub fn build_resolver(world: &World, pairing: &Pairing, origin: DVec3) -> SubLevelResolver {
    let mut rows = Vec::with_capacity(pairing.pairs.len() * 2);
    for (a, b) in &pairing.pairs {
        for (portal, partner) in [(a, b), (b, a)] {
            let Some((pose, half)) = pose_of(world, portal, origin) else {
                continue;
            };
            rows.push((
                portal.instance,
                PortalRow::new(Some(partner.instance), pose, half),
            ));
        }
    }
    let mut resolver = SubLevelResolver::new();
    resolver.insert_sublevel(0, SubLevelContents::new([], rows));
    resolver
}

/// Builds the frame's chains each frame and publishes them.
pub struct PortalSync {
    /// Routes up to the settings' depth and count.
    bridge: PortalProjectionBridge,
    /// Problems already reported, so a standing problem is logged once.
    reported: HashSet<PortalProblem>,
    chains: Arc<PortalChainSet>,
}

impl Default for PortalSync {
    /// With the project's portal settings.
    fn default() -> Self {
        use engine_backend::subsystems::render::helio_renderer::project_settings;
        Self::with_settings(&PortalSettings::from_settings(&project_settings::resolved(
            &helio_pass_portal_cull::settings::PORTALS,
        )))
    }
}

impl PortalSync {
    /// Projecting routes as deep and as many as `settings` allow.
    pub fn with_settings(settings: &PortalSettings) -> Self {
        let config = PortalProjectionConfig {
            max_chain_depth: settings.max_chain_depth.max(1),
            max_chains: Some(settings.max_chains.max(1)),
            ..PortalProjectionConfig::default()
        };
        Self {
            bridge: PortalProjectionBridge::with_config(config).expect("a nonzero chain depth"),
            reported: HashSet::new(),
            chains: Arc::new(PortalChainSet::default()),
        }
    }

    /// Rebuild the chains of `world` (relative to `origin`).
    pub fn rebuild(&mut self, world: &World, origin: DVec3) -> PortalChainSet {
        let pairing = pair_portals(world);
        let current: HashSet<PortalProblem> = pairing.problems.iter().copied().collect();
        for problem in &current {
            if self.reported.insert(*problem) {
                tracing::warn!("{problem}");
            }
        }
        self.reported.retain(|problem| current.contains(problem));
        if pairing.pairs.is_empty() {
            return PortalChainSet::default();
        }
        let resolver = build_resolver(world, &pairing, origin);
        match self.bridge.build(&resolver) {
            Ok(frame) => chain_set(&frame),
            Err(error) => {
                tracing::warn!("portals not drawn: {error}");
                PortalChainSet::default()
            }
        }
    }

    /// The chains last published.
    pub fn chains(&self) -> &Arc<PortalChainSet> {
        &self.chains
    }
}

impl SpaceProducer for PortalSync {
    fn apply(
        &mut self,
        world: &World,
        origin: DVec3,
        _table: &mut SpaceTable,
        payloads: &mut helio_core::ScenePayloads,
    ) {
        let chains = self.rebuild(world, origin);
        if *self.chains != chains {
            self.chains = Arc::new(chains);
        }
        payloads.insert(&PORTAL_CHAIN_SET, self.chains.clone());
    }

    fn derivation(
        &self,
        _device: &wgpu::Device,
    ) -> Option<Box<dyn helio_scene_join::SceneDerivation>> {
        None
    }
}
