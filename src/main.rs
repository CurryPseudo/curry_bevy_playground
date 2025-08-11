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
use bevy::input::mouse::{MouseMotion, MouseWheel, MouseScrollUnit};
use bevy::window::{PrimaryWindow, Window, WindowPlugin};
use rand::Rng;
use bevy_egui::{egui, EguiContexts, EguiPlugin, EguiContextPass};

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    canvas: Some("#bevy".into()),
                    fit_canvas_to_parent: true,
                    ..Default::default()
                }),
                ..Default::default()
            }),
            EguiPlugin { enable_multipass_for_primary_context: true },
        ))
        .init_resource::<SunAngles>()
        .add_systems(Startup, (setup_camera_fog, setup_terrain_scene))
        .add_systems(EguiContextPass, sun_angles_ui)
        .add_systems(Update, (
            apply_sun_angles,
            camera_grab_pointer,
            camera_look,
            camera_move,
        ))
        .run();
}

fn setup_camera_fog(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        Camera {
            hdr: true,
            ..default()
        },
        Transform::from_xyz(-1.2, 0.15, 0.0).looking_at(Vec3::Y * 0.1, Vec3::Y),
        Atmosphere::EARTH,
        AtmosphereSettings {
            aerial_view_lut_max_distance: 3.2e5,
            scene_units_to_m: 1e+4,
            ..Default::default()
        },
        Exposure::SUNLIGHT,
        Tonemapping::AcesFitted,
        Bloom::NATURAL,
        EditorCameraController::default(),
    ));
}

#[derive(Component)]
struct Terrain;

fn setup_terrain_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let cascade_shadow_config = CascadeShadowConfigBuilder {
        first_cascade_far_bound: 0.3,
        maximum_distance: 3.0,
        ..default()
    }
    .build();

    commands.spawn((
        DirectionalLight {
            shadows_enabled: true,
            illuminance: lux::RAW_SUNLIGHT,
            ..default()
        },
        Transform::from_xyz(1.0, -0.4, 0.0).looking_at(Vec3::ZERO, Vec3::Y),
        cascade_shadow_config,
    ));

    let sphere_mesh = meshes.add(Mesh::from(Sphere { radius: 1.0 }));

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

// --- Sun control ---
#[derive(Resource)]
struct SunAngles {
    // degrees
    azimuth_deg: f32,   // 0..=360, measured from +X towards +Z
    elevation_deg: f32, // -90..=90
}

impl Default for SunAngles {
    fn default() -> Self {
        Self { azimuth_deg: 0.0, elevation_deg: -30.0 }
    }
}

fn sun_angles_ui(mut contexts: EguiContexts, mut angles: ResMut<SunAngles>) {
    egui::Window::new("天光设置").show(contexts.ctx_mut(), |ui| {
        ui.label("使用滑块调整方向光角度");
        ui.add(egui::Slider::new(&mut angles.azimuth_deg, 0.0..=360.0).text("方位角(°)"));
        ui.add(egui::Slider::new(&mut angles.elevation_deg, -90.0..=89.0).text("仰角(°)"));
    });
}

fn apply_sun_angles(angles: Res<SunAngles>, mut suns: Query<&mut Transform, With<DirectionalLight>>) {
    let Ok(mut tf) = suns.single_mut() else { return; };
    let az = angles.azimuth_deg.to_radians();
    let el = angles.elevation_deg.to_radians();
    let dir = Vec3::new(el.cos() * az.cos(), el.sin(), el.cos() * az.sin());

    let distance = 1.0;
    tf.translation = -dir * distance;
    tf.look_at(Vec3::ZERO, Vec3::Y);
}

// --- Editor-style free fly camera ---
#[derive(Component)]
struct EditorCameraController {
    yaw_radians: f32,
    pitch_radians: f32,
    base_speed_units_per_second: f32,
    mouse_sensitivity_radians_per_pixel: f32,
}

impl Default for EditorCameraController {
    fn default() -> Self {
        Self {
            yaw_radians: 0.0,
            pitch_radians: 0.0,
            base_speed_units_per_second: 2.0,
            mouse_sensitivity_radians_per_pixel: 0.0025,
        }
    }
}

fn camera_grab_pointer(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mut window_q: Query<&mut Window, With<PrimaryWindow>>,
    mut contexts: EguiContexts,
) {
    let Ok(mut window) = window_q.single_mut() else { return; };
    let ctx = contexts.ctx_mut();
    if ctx.wants_pointer_input() || ctx.wants_keyboard_input() { return; }
    let want_lock = mouse_buttons.pressed(MouseButton::Right);
    use bevy::window::CursorGrabMode;
    if want_lock {
        if window.cursor_options.grab_mode != CursorGrabMode::Locked {
            window.cursor_options.grab_mode = CursorGrabMode::Locked;
        }
        if window.cursor_options.visible {
            window.cursor_options.visible = false;
        }
    } else {
        if window.cursor_options.grab_mode != CursorGrabMode::None {
            window.cursor_options.grab_mode = CursorGrabMode::None;
        }
        if !window.cursor_options.visible {
            window.cursor_options.visible = true;
        }
    }
}

fn camera_look(
    mut mouse_motion_events: EventReader<MouseMotion>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mut query: Query<&mut EditorCameraController, With<Camera3d>>, 
    mut xform_q: Query<&mut Transform, With<Camera3d>>,
    mut contexts: EguiContexts,
) {
    if !mouse_buttons.pressed(MouseButton::Right) { return; }
    let ctx = contexts.ctx_mut();
    if ctx.wants_pointer_input() || ctx.wants_keyboard_input() { return; }
    let Ok(mut controller) = query.single_mut() else { return; };
    let Ok(mut transform) = xform_q.single_mut() else { return; };

    let mut delta = Vec2::ZERO;
    for ev in mouse_motion_events.read() { delta += ev.delta; }
    if delta == Vec2::ZERO { return; }

    controller.yaw_radians -= delta.x * controller.mouse_sensitivity_radians_per_pixel;
    controller.pitch_radians -= delta.y * controller.mouse_sensitivity_radians_per_pixel;

    let half_pi = PI * 0.5 - 0.001;
    controller.pitch_radians = controller.pitch_radians.clamp(-half_pi, half_pi);

    let yaw = Quat::from_rotation_y(controller.yaw_radians);
    let pitch = Quat::from_rotation_x(controller.pitch_radians);
    transform.rotation = yaw * pitch;
}

fn camera_move(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut controller_q: Query<&mut EditorCameraController, With<Camera3d>>,
    mut transform_q: Query<&mut Transform, With<Camera3d>>,
    mut wheel_events: EventReader<MouseWheel>,
    mut contexts: EguiContexts,
) {
    let ctx = contexts.ctx_mut();
    if ctx.wants_pointer_input() || ctx.wants_keyboard_input() { return; }
    let Ok(mut controller) = controller_q.single_mut() else { return; };
    let Ok(mut transform) = transform_q.single_mut() else { return; };

    // Adjust and persist base speed with mouse wheel
    let mut base_speed = controller.base_speed_units_per_second;
    for ev in wheel_events.read() {
        let scroll = match ev.unit {
            MouseScrollUnit::Line => ev.y,
            MouseScrollUnit::Pixel => ev.y * 0.05,
        };
        base_speed = (base_speed * (1.0 + scroll * 0.1)).max(0.01);
    }
    controller.base_speed_units_per_second = base_speed;

    // Modifiers
    let mut speed = base_speed;
    if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) {
        speed *= 5.0;
    }
    if keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight) {
        speed *= 0.2;
    }

    // Movement input
    let mut input_direction = Vec3::ZERO;
    if keys.pressed(KeyCode::KeyW) { input_direction += Vec3::Z; }
    if keys.pressed(KeyCode::KeyS) { input_direction += -Vec3::Z; }
    if keys.pressed(KeyCode::KeyA) { input_direction += -Vec3::X; }
    if keys.pressed(KeyCode::KeyD) { input_direction += Vec3::X; }
    if keys.pressed(KeyCode::KeyE) || keys.pressed(KeyCode::Space) { input_direction += Vec3::Y; }
    if keys.pressed(KeyCode::KeyQ) { input_direction += -Vec3::Y; }

    if input_direction == Vec3::ZERO { return; }

    let forward = transform.forward();
    let right = transform.right();
    let up = Vec3::Y;
    let world_dir = (forward * input_direction.z
        + right * input_direction.x
        + up * input_direction.y)
        .normalize();

    transform.translation += world_dir * speed * time.delta_secs();
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