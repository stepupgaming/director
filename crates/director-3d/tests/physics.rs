//! Baked rigid-body physics on the CPU: determinism, stacking, toppling,
//! spawning, kinematic clips and the checks that refuse bad input.

use std::sync::atomic::{AtomicU64, Ordering};

use bevy::math::{Quat, Vec3};
use director_3d::{Physics, PhysicsSource};
use director_scene::{BodyKind, BodyShape, ColliderKind, ModelPhysics, PhysicsBody, PhysicsSpawner, SpawnEmitter};

fn fixture() -> Vec<u8> {
    std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/physics.glb")).unwrap()
}

/// A model hash no other bake in this process shares, so each call
/// simulates afresh.
fn fresh() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(0x5eed_0000);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

fn bake(spec: ModelPhysics) -> Result<Physics, String> {
    let bytes = fixture();
    let info = director_3d::inspect(&bytes).unwrap();
    Physics::new(&bytes, fresh(), spec, info.duration, false)
}

fn node(name: &str, kind: BodyKind) -> PhysicsBody {
    PhysicsBody { node: Some(name.into()), kind, ..Default::default() }
}

fn ground() -> PhysicsBody {
    node("Ground", BodyKind::Static)
}

fn track(physics: &Physics, name: &str) -> usize {
    physics.bake.tracks.iter().position(|t| t.source == PhysicsSource::Node(name.into())).unwrap_or_else(|| panic!("no track for {name}"))
}

fn tilt(q: Quat) -> f32 {
    (q * Vec3::Y).angle_between(Vec3::Y).to_degrees()
}

fn coins(count: u32) -> ModelPhysics {
    ModelPhysics {
        duration: 5.0,
        bodies: vec![ground()],
        spawners: vec![PhysicsSpawner {
            body: PhysicsBody { node: Some("Coin".into()), position: [0.0, 0.6, 0.0], restitution: 0.3, ..Default::default() },
            count,
            start: 0.0,
            duration: 2.0,
            emitter: SpawnEmitter::Sphere,
            radius: 0.15,
            velocity_jitter: 0.3,
            spin_jitter: 720.0,
            scale_jitter: 0.2,
            ..Default::default()
        }],
        ..Default::default()
    }
}

#[test]
fn the_same_inputs_bake_bit_identical_tracks() {
    let spec = coins(60);
    let (a, b) = (bake(spec.clone()).unwrap(), bake(spec).unwrap());
    assert!(!std::sync::Arc::ptr_eq(&a.bake, &b.bake), "both bakes simulated");
    assert_eq!(a.bake.tracks.len(), b.bake.tracks.len());
    for (x, y) in a.bake.tracks.iter().zip(&b.bake.tracks) {
        assert_eq!((x.birth, x.scale.to_bits()), (y.birth, y.scale.to_bits()));
        let bits = |t: &director_3d::PhysicsTrack| t.poses.iter().flatten().map(|v| v.to_bits()).collect::<Vec<_>>();
        assert_eq!(bits(x), bits(y));
    }
    // A cached bake of the same inputs is the same bake.
    let bytes = fixture();
    let spec = coins(60);
    let one = Physics::new(&bytes, 7, spec.clone(), 1.0, false).unwrap();
    let two = Physics::new(&bytes, 7, spec, 1.0, false).unwrap();
    assert!(std::sync::Arc::ptr_eq(&one.bake, &two.bake));
}

#[test]
fn poses_depend_only_on_the_time_asked() {
    let physics = bake(coins(20)).unwrap();
    let bake = &physics.bake;
    let times: Vec<f64> = (0..60).map(|i| i as f64 / 12.0).collect();
    let forward: Vec<_> = times.iter().map(|&t| (0..bake.tracks.len()).map(|i| bake.pose(i, t)).collect::<Vec<_>>()).collect();
    let backward: Vec<_> = times.iter().rev().map(|&t| (0..bake.tracks.len()).map(|i| bake.pose(i, t)).collect::<Vec<_>>()).collect();
    assert!(forward.iter().eq(backward.iter().rev()));
    // A copy is unborn before its birth and between steps blends its
    // neighbours.
    let last = bake.tracks.len() - 1;
    assert!(bake.pose(last, 0.0).is_none());
    assert!(bake.pose(last, 4.0).is_some());
    let (p0, _) = bake.pose(0, 0.5).unwrap();
    let (p1, _) = bake.pose(0, 0.5 + 1.0 / 120.0).unwrap();
    let (mid, _) = bake.pose(0, 0.5 + 0.5 / 120.0).unwrap();
    assert!(mid.distance((p0 + p1) / 2.0) < 1e-5);
}

#[test]
fn a_stack_of_ten_crates_stays_standing() {
    let spec = ModelPhysics { duration: 4.0, bodies: vec![ground(), node("Crate.*", BodyKind::Dynamic)], ..Default::default() };
    let physics = bake(spec).unwrap();
    for i in 0..10 {
        let t = track(&physics, &format!("Crate.{i}"));
        let (p, q) = physics.bake.pose(t, 4.0).unwrap();
        let rest = Vec3::new(-1.5, 0.1 + 0.2 * i as f32, 0.0);
        assert!(p.distance(rest) < 0.01, "Crate.{i} drifted to {p}");
        assert!(tilt(q) < 1.0, "Crate.{i} tilted {} degrees", tilt(q));
    }
    assert!(physics.bake.stats.penetration < 0.005, "{:?}", physics.bake.stats);
}

/// The dominoes' bake: the pusher's clip tips the first.
fn dominoes() -> Physics {
    let spec = ModelPhysics {
        duration: 4.0,
        bodies: vec![
            ground(),
            node("Domino.?", BodyKind::Dynamic),
            PhysicsBody { node: Some("Pusher".into()), kind: BodyKind::Kinematic, collider: ColliderKind::Box, ..Default::default() },
        ],
        ..Default::default()
    };
    bake(spec).unwrap()
}

#[test]
fn a_row_of_dominoes_topples_in_order() {
    let physics = dominoes();
    let fell: Vec<f64> = (0..8)
        .map(|i| {
            let t = track(&physics, &format!("Domino.{i}"));
            (0..=480)
                .map(|s| s as f64 / 120.0)
                .find(|&s| tilt(physics.bake.pose(t, s).unwrap().1) > 30.0)
                .unwrap_or_else(|| panic!("Domino.{i} never fell"))
        })
        .collect();
    assert!(fell.windows(2).all(|w| w[0] < w[1]), "fall times {fell:?}");
}

#[test]
fn a_kinematic_pusher_follows_its_clip() {
    let physics = dominoes();
    // The clip drives the pusher through Bevy's own animation, so it has no
    // track, and the first domino stands until the pusher reaches it.
    assert!(!physics.bake.tracks.iter().any(|t| t.source == PhysicsSource::Node("Pusher".into())));
    let first = track(&physics, "Domino.0");
    let (rest, _) = physics.bake.pose(first, 0.0).unwrap();
    let (before, q) = physics.bake.pose(first, 0.7).unwrap();
    assert!(before.distance(rest) < 1e-4 && tilt(q) < 0.1, "moved early to {before}");
    let (_, q) = physics.bake.pose(first, 1.0).unwrap();
    assert!(tilt(q) > 2.0, "untouched at 1 s: {} degrees", tilt(q));
}

#[test]
fn spawned_coins_come_to_rest_without_sinking() {
    let physics = bake(coins(120)).unwrap();
    let bake = &physics.bake;
    let copies: Vec<usize> = (0..bake.tracks.len()).filter(|&i| matches!(bake.tracks[i].source, PhysicsSource::Copy(0))).collect();
    assert_eq!(copies.len(), 120);
    let births: Vec<u32> = copies.iter().map(|&i| bake.tracks[i].birth).collect();
    assert!(births.windows(2).all(|w| w[0] <= w[1]) && births[0] == 0 && *births.last().unwrap() > 200);
    // Coins that land on their rim may roll off the 6 m floor; every other
    // coin rests on it.
    let on_floor: Vec<usize> = copies.iter().copied().filter(|&i| bake.pose(i, 5.0).unwrap().0.y > -0.05).collect();
    assert!(on_floor.len() >= 110, "{} of 120 on the floor", on_floor.len());
    // A pile is chaotic: a few coins may still roll on their rims or slide
    // off its edge at the end, as real ones do.
    let mut moving = Vec::new();
    for &i in &on_floor {
        let (end, _) = bake.pose(i, 5.0).unwrap();
        let (before, _) = bake.pose(i, 4.9).unwrap();
        if end.distance(before) >= 2e-3 {
            moving.push((i, before, end));
        }
        assert!(end.y > 0.006 * bake.tracks[i].scale - 5e-4, "copy {i} sank to {end}");
    }
    assert!(moving.len() <= 6, "{} coins still moving: {moving:?}", moving.len());
    // A pair of thin coins may cross in flight; nearly every contact stays
    // shallow.
    assert!(bake.stats.deep <= 2, "{:?}", bake.stats);
    // Rested bodies keep one pose for their rest.
    assert!(bake.stats.sleeping > 0);
    assert!(copies.iter().any(|&i| bake.tracks[i].poses.len() < 600 - bake.tracks[i].birth as usize));
}

#[test]
fn built_in_shapes_fall_and_rest() {
    let spec = ModelPhysics {
        duration: 3.0,
        bodies: vec![
            PhysicsBody { shape: Some(BodyShape::Box), kind: BodyKind::Static, size: [4.0, 0.2, 4.0], position: [0.0, -0.1, 0.0], ..Default::default() },
            PhysicsBody { shape: Some(BodyShape::Sphere), name: Some("Ball".into()), size: [0.2; 3], position: [0.0, 1.0, 0.0], restitution: 0.0, ..Default::default() },
        ],
        ..Default::default()
    };
    let physics = bake(spec).unwrap();
    assert_eq!(physics.bake.fixed, vec![0]);
    let (p, _) = physics.bake.pose(0, 3.0).unwrap();
    assert!((p.y - 0.1).abs() < 0.005, "ball rests at {p}");
    assert_eq!(director_3d::physics_shape_names(&physics.spec), vec!["Ball".to_string()]);
}

#[test]
fn patterns_match_names() {
    use director_3d::body_pattern_matches as m;
    assert!(m("Crate.*", "Crate.12") && m("Domino.?", "Domino.3") && m("*", "") && m("a*b*c", "aXbYc"));
    assert!(!m("Domino.?", "Domino.10") && !m("Crate.*", "Crates") && !m("a*b", "ab c"));
}

#[test]
fn bad_physics_is_refused() {
    let err = |bodies: Vec<PhysicsBody>| bake(ModelPhysics { bodies, ..Default::default() }).unwrap_err();
    assert!(err(vec![node("Barrel", BodyKind::Dynamic)]).contains("names no node"));
    let trimesh = PhysicsBody { collider: ColliderKind::Trimesh, ..node("Crate.0", BodyKind::Dynamic) };
    assert!(err(vec![trimesh]).contains("trimesh needs a static body"));
    assert!(err(vec![node("Pusher", BodyKind::Dynamic)]).contains("make it kinematic"));
    assert!(err(vec![node("Ground", BodyKind::Dynamic)]).contains("collider box"));
    assert!(err(vec![node("Crate.*", BodyKind::Dynamic), node("Crate.1", BodyKind::Static)]).contains("matched by two"));
    assert!(err(vec![node("Camera", BodyKind::Static)]).contains("no mesh"));
    assert!(err(vec![]).contains("needs a <rigid> or <spawner>"));
    let both = PhysicsBody { shape: Some(BodyShape::Box), ..node("Crate.0", BodyKind::Dynamic) };
    assert!(err(vec![both]).contains("not both"));
    let late = ModelPhysics { spawners: vec![PhysicsSpawner { start: 6.0, ..coins(1).spawners[0].clone() }], ..coins(1) };
    assert!(bake(late).unwrap_err().contains("within the simulation"));
    let long = ModelPhysics { duration: 0.0, ..coins(1) };
    assert!(bake(long).unwrap_err().contains("duration"));
}

/// Bake time and track memory of 300 coins over 5 s, printed for the plan's
/// measurement table.
#[test]
fn three_hundred_coins_bake_in_seconds() {
    let mut spec = coins(300);
    spec.spawners[0].emitter = SpawnEmitter::Box;
    spec.spawners[0].extent = [0.3, 0.6, 0.3];
    spec.spawners[0].body.position = [0.0, 0.9, 0.0];
    let physics = bake(spec).unwrap();
    let stats = physics.bake.stats;
    println!(
        "300 coins over 5 s: {:.0} ms bake, {} steps, {} bodies, {} asleep, {:.1} mm deepest contact, {} deeper than 2 mm, {:.2} MB of tracks",
        stats.millis,
        physics.bake.steps,
        stats.bodies,
        stats.sleeping,
        stats.penetration * 1000.0,
        stats.deep,
        stats.bytes as f64 / 1e6
    );
    assert!(stats.millis < 60_000.0);
    assert!(stats.bytes < 300 * 601 * 28 + 1);
}
