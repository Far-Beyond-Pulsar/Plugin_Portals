//! Pairing authored portals and the projection a renderer runs for them
//! (`portals_component::projection`): two portals with one Link ID are
//! partners, one alone or three are reported, a hidden or disabled portal
//! leaves its partner alone, and the projection takes its spaces from the
//! shared table. Moved from the component crate's unit tests: attaching a
//! portal needs its class registered, which the root does.

// The plugin as a game links it: its root, with every registration.
use portals_plugin as _;

use std::sync::Arc;

use engine_backend::scene::{SpaceProducer, SpaceTable};
use glam::{DVec3, Mat4};
use helio_pass_portal_cull::PORTAL_COUNTS;
use portals_component::{
    PortalComponent as AuthoredPortal, PortalProblem, PortalSync, pair_portals,
};
use pulsar_scene_model::world_ext::{SceneWorldExt, SpawnObject};
use pulsar_scene_model::{Transform, Visibility};
use pulsar_scenedb::{Entity, World};

fn portal_at(world: &mut World, link_id: i32, position: [f64; 3]) -> Entity {
    let object = world
        .spawn_object(SpawnObject::new("portal").with_transform(Transform {
            position,
            ..Transform::default()
        }))
        .unwrap();
    pulsar_world_registry::attach_value(
        world,
        object,
        AuthoredPortal {
            link_id,
            ..AuthoredPortal::default()
        },
    )
    .unwrap();
    object
}

#[test]
fn two_portals_with_one_link_id_are_partners() {
    let mut world = World::new();
    portal_at(&mut world, 5, [0.0; 3]);
    portal_at(&mut world, 5, [20.0, 0.0, 0.0]);
    portal_at(&mut world, 0, [1.0, 0.0, 0.0]);
    let pairing = pair_portals(&world);
    assert_eq!(
        pairing.pairs.len(),
        1,
        "an unlinked portal pairs with nothing"
    );
    assert!(pairing.problems.is_empty());
}

#[test]
fn one_or_three_portals_with_an_id_are_reported() {
    let mut world = World::new();
    portal_at(&mut world, 1, [0.0; 3]);
    for _ in 0..3 {
        portal_at(&mut world, 2, [0.0; 3]);
    }
    let pairing = pair_portals(&world);
    assert!(pairing.pairs.is_empty());
    assert_eq!(
        pairing.problems,
        [
            PortalProblem::Alone { link_id: 1 },
            PortalProblem::Crowded {
                link_id: 2,
                count: 3
            }
        ]
    );
}

#[test]
fn a_hidden_or_disabled_portal_leaves_its_partner_alone() {
    let mut world = World::new();
    let a = portal_at(&mut world, 7, [0.0; 3]);
    portal_at(&mut world, 7, [5.0, 0.0, 0.0]);
    assert_eq!(pair_portals(&world).pairs.len(), 1);
    world.get_mut::<Visibility>(a).unwrap().visible = false;
    let pairing = pair_portals(&world);
    assert!(pairing.pairs.is_empty());
    assert_eq!(pairing.problems, [PortalProblem::Alone { link_id: 7 }]);
}

#[test]
fn the_projection_gets_views_and_chains_through_the_shared_table() {
    let mut world = World::new();
    portal_at(&mut world, 3, [0.0, 0.0, 0.0]);
    portal_at(&mut world, 3, [50.0, 0.0, 0.0]);
    let mut sync = PortalSync::default();
    let mut table = SpaceTable::new(vec![Mat4::IDENTITY, Mat4::IDENTITY]);
    let mut payloads = helio_core::ScenePayloads::default();
    sync.apply(&world, DVec3::ZERO, &mut table, &mut payloads);
    let counts = payloads
        .get(&PORTAL_COUNTS)
        .expect("the counts are published");
    let (views, chains) = (counts.views, counts.chains);
    assert!(
        views >= 2 && chains >= 2,
        "a view and a chain per direction ({views}, {chains})"
    );
    assert!(
        table.matrices().len() > 2,
        "the projection's spaces came after the frame slot"
    );
    let mut again = helio_core::ScenePayloads::default();
    sync.apply(
        &world,
        DVec3::ZERO,
        &mut SpaceTable::new(vec![Mat4::IDENTITY, Mat4::IDENTITY]),
        &mut again,
    );
    assert!(
        Arc::ptr_eq(&counts, &again.get(&PORTAL_COUNTS).unwrap()),
        "unchanged counts are the same payload"
    );
}
