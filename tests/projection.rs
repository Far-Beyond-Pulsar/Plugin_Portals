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
use helio_pass_portal_cull::PORTAL_OPENINGS;
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
fn a_pair_publishes_an_opening_each_way_and_takes_no_table_slot() {
    let mut world = World::new();
    portal_at(&mut world, 3, [0.0, 0.0, 0.0]);
    portal_at(&mut world, 3, [50.0, 0.0, 0.0]);
    let mut sync = PortalSync::default();
    let mut table = SpaceTable::new(vec![Mat4::IDENTITY, Mat4::IDENTITY]);
    let mut payloads = helio_core::ScenePayloads::default();
    sync.apply(&world, DVec3::ZERO, &mut table, &mut payloads);
    let openings = payloads
        .get(&PORTAL_OPENINGS)
        .expect("the openings are published");
    assert_eq!(openings.0.len(), 2, "one opening per direction");
    // Each opening's view map places what it shows beyond it: the partner's
    // position lands at its own.
    for opening in &openings.0 {
        let here = opening.transform().w_axis.truncate();
        let partner = openings
            .0
            .iter()
            .find(|other| other.key != opening.key)
            .unwrap()
            .transform()
            .w_axis
            .truncate();
        assert!(opening.content_to_drawn.transform_point3(partner).distance(here) < 1e-3);
    }
    // What a portal shows is a view of the frame, not content placed in a
    // coordinate space: the frame's table keeps only its own slots.
    assert_eq!(table.matrices().len(), 2);
    let mut again = helio_core::ScenePayloads::default();
    sync.apply(
        &world,
        DVec3::ZERO,
        &mut SpaceTable::new(vec![Mat4::IDENTITY, Mat4::IDENTITY]),
        &mut again,
    );
    assert!(
        Arc::ptr_eq(&openings, &again.get(&PORTAL_OPENINGS).unwrap()),
        "unchanged openings are the same payload"
    );
}
