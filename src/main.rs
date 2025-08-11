//! This example showcases pbr atmospheric scattering

use std::f32::consts::PI;

use bevy::{
    core_pipeline::{bloom::Bloom, tonemapping::Tonemapping},
    pbr::{light_consts::lux, Atmosphere, AtmosphereSettings, CascadeShadowConfigBuilder},
    prelude::*,
    render::camera::Exposure,
};
use bevy::render::mesh::Indices;
use bevy::asset::RenderAssetUsages;
use bevy::render::render_resource::PrimitiveTopology;
use rand::Rng;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_systems(Startup, (setup_camera_fog, setup_terrain_scene))
        .add_systems(Update, dynamic_scene)
        .run();
}

fn setup_camera_fog(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        // HDR is required for atmospheric scattering to be properly applied to the scene
        Camera {
            hdr: true,
            ..default()
        },
        Transform::from_xyz(-1.2, 0.15, 0.0).looking_at(Vec3::Y * 0.1, Vec3::Y),
        // This is the component that enables atmospheric scattering for a camera
        Atmosphere::EARTH,
        // The scene is in units of 10km, so we need to scale up the
        // aerial view lut distance and set the scene scale accordingly.
        // Most usages of this feature will not need to adjust this.
        AtmosphereSettings {
            aerial_view_lut_max_distance: 3.2e5,
            scene_units_to_m: 1e+4,
            ..Default::default()
        },
        // The directional light illuminance  used in this scene
        // (the one recommended for use with this feature) is
        // quite bright, so raising the exposure compensation helps
        // bring the scene to a nicer brightness range.
        Exposure::SUNLIGHT,
        // Tonemapper chosen just because it looked good with the scene, any
        // tonemapper would be fine :)
        Tonemapping::AcesFitted,
        // Bloom gives the sun a much more natural look.
        Bloom::NATURAL,
    ));
}

#[derive(Component)]
struct Terrain;

fn setup_terrain_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Configure a properly scaled cascade shadow map for this scene (defaults are too large, mesh units are in km)
    let cascade_shadow_config = CascadeShadowConfigBuilder {
        first_cascade_far_bound: 0.3,
        maximum_distance: 3.0,
        ..default()
    }
    .build();

    // Sun
    commands.spawn((
        DirectionalLight {
            shadows_enabled: true,
            // lux::RAW_SUNLIGHT is recommended for use with this feature, since
            // other values approximate sunlight *post-scattering* in various
            // conditions. RAW_SUNLIGHT in comparison is the illuminance of the
            // sun unfiltered by the atmosphere, so it is the proper input for
            // sunlight to be filtered by the atmosphere.
            illuminance: lux::RAW_SUNLIGHT,
            ..default()
        },
        Transform::from_xyz(1.0, -0.4, 0.0).looking_at(Vec3::ZERO, Vec3::Y),
        cascade_shadow_config,
    ));

    let sphere_mesh = meshes.add(Mesh::from(Sphere { radius: 1.0 }));

    // light probe spheres
    commands.spawn((
        Mesh3d(sphere_mesh.clone()),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::WHITE,
            metallic: 1.0,
            perceptual_roughness: 0.0,
            ..default()
        })),
        Transform::from_xyz(-0.3, 0.1, -0.1).with_scale(Vec3::splat(0.05)),
    ));

    commands.spawn((
        Mesh3d(sphere_mesh.clone()),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::WHITE,
            metallic: 0.0,
            perceptual_roughness: 1.0,
            ..default()
        })),
        Transform::from_xyz(-0.3, 0.1, 0.1).with_scale(Vec3::splat(0.05)),
    ));

    // Terrain (generated at runtime)
    let terrain_mesh = generate_random_terrain(128, 128, 4.0, 4.0, 0.25);
    let terrain_handle = meshes.add(terrain_mesh);

    commands.spawn((
        Terrain,
        Mesh3d(terrain_handle),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.45, 0.55, 0.4),
            metallic: 0.0,
            perceptual_roughness: 1.0,
            ..default()
        })),
        Transform::from_xyz(0.0, -0.2, 0.0)
            .with_scale(Vec3::splat(0.5))
            .with_rotation(Quat::from_rotation_y(PI / 2.0)),
    ));
}

fn dynamic_scene(mut suns: Query<&mut Transform, With<DirectionalLight>>, time: Res<Time>) {
    suns.iter_mut()
        .for_each(|mut tf| tf.rotate_x(-time.delta_secs() * PI / 10.0));
}

fn generate_random_terrain(
    num_x: usize,
    num_z: usize,
    size_x: f32,
    size_z: f32,
    height_scale: f32,
) -> Mesh {
    let vertex_count = num_x * num_z;
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(vertex_count);
    let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(vertex_count);

    let dx = if num_x > 1 { size_x / (num_x as f32 - 1.0) } else { 0.0 };
    let dz = if num_z > 1 { size_z / (num_z as f32 - 1.0) } else { 0.0 };

    let x_origin = -size_x * 0.5;
    let z_origin = -size_z * 0.5;

    let mut rng = rand::thread_rng();
    // Randomized wave parameters for smooth terrain
    let waves: Vec<(f32, f32, f32, f32, f32)> = (0..4)
        .map(|i| {
            let fi = i as f32 + 1.0;
            let fx = rng.gen_range(0.5..3.0) * fi;
            let fz = rng.gen_range(0.5..3.0) * fi;
            let px = rng.gen_range(0.0..(2.0 * PI));
            let pz = rng.gen_range(0.0..(2.0 * PI));
            let amp = height_scale / fi;
            (fx, fz, px, pz, amp)
        })
        .collect();

    for iz in 0..num_z {
        for ix in 0..num_x {
            let x = x_origin + ix as f32 * dx;
            let z = z_origin + iz as f32 * dz;

            let mut h = 0.0f32;
            for (fx, fz, px, pz, amp) in &waves {
                h += ((x * fx + px).sin() * (z * fz + pz).cos()) * *amp;
            }
            // Gentle bias to keep floor near y=0
            h *= 0.8;

            positions.push([x, h, z]);
            uvs.push([
                ix as f32 / (num_x as f32 - 1.0),
                iz as f32 / (num_z as f32 - 1.0),
            ]);
        }
    }

    let quad_count_x = if num_x > 1 { num_x - 1 } else { 0 };
    let quad_count_z = if num_z > 1 { num_z - 1 } else { 0 };
    let mut indices: Vec<u32> =
        Vec::with_capacity(quad_count_x * quad_count_z * 6);

    for iz in 0..quad_count_z {
        for ix in 0..quad_count_x {
            let i0 = (iz * num_x + ix) as u32;
            let i1 = i0 + 1;
            let i2 = i0 + num_x as u32;
            let i3 = i2 + 1;
            // Two triangles per quad, CCW winding
            indices.extend_from_slice(&[i0, i2, i1, i1, i2, i3]);
        }
    }

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(indices));

    // Compute normals for proper PBR lighting
    mesh.compute_normals();

    mesh
}