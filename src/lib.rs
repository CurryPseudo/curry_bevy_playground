use std::f32::consts::PI;

use bevy::asset::RenderAssetUsages;
use bevy::input::mouse::{MouseMotion, MouseScrollUnit, MouseWheel};
use bevy::render::mesh::Indices;
use bevy::render::render_resource::PrimitiveTopology;
use bevy::window::{PrimaryWindow, Window};
use bevy::{
    core_pipeline::{bloom::Bloom, tonemapping::Tonemapping},
    pbr::{light_consts::lux, Atmosphere, AtmosphereSettings, CascadeShadowConfigBuilder},
    prelude::*,
    render::camera::Exposure,
};
use bevy_egui::{egui, EguiContexts, EguiPlugin, EguiPrimaryContextPass, EguiStartupSet};
use bevy_inspector_egui::quick::WorldInspectorPlugin;
use image::RgbaImage;
use rand::Rng;
#[cfg(target_arch = "wasm32")]
use rfd::AsyncFileDialog;
#[cfg(not(target_arch = "wasm32"))]
use rfd::FileDialog;
#[cfg(target_arch = "wasm32")]
use std::sync::{Mutex, OnceLock};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen_futures::spawn_local;

#[cfg(target_arch = "wasm32")]
static HEIGHTMAP_QUEUE: OnceLock<Mutex<Vec<Vec<u8>>>> = OnceLock::new();
use bevy::app::{App, Plugin};

pub struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((EguiPlugin::default(), WorldInspectorPlugin::default()))
            .init_resource::<SunAngles>()
            .init_resource::<HeightmapUiState>()
            .add_systems(
                PreStartup,
                setup_camera_fog.before(EguiStartupSet::InitContexts),
            )
            .add_systems(Startup, init_heightmap_queue_if_wasm)
            .add_systems(Startup, (setup_egui_cjk_font, setup_terrain_scene))
            .add_systems(EguiPrimaryContextPass, (sun_angles_ui, heightmap_ui))
            .add_systems(
                Update,
                (
                    apply_sun_angles,
                    camera_grab_pointer,
                    camera_look,
                    camera_move,
                ),
            );
    }
}

#[cfg(target_arch = "wasm32")]
fn init_heightmap_queue_if_wasm() {
    let _ = HEIGHTMAP_QUEUE.get_or_init(|| Mutex::new(Vec::new()));
}

#[cfg(not(target_arch = "wasm32"))]
fn init_heightmap_queue_if_wasm() {}

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
        Self {
            azimuth_deg: 0.0,
            elevation_deg: -30.0,
        }
    }
}

fn sun_angles_ui(mut contexts: EguiContexts, mut angles: ResMut<SunAngles>) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    egui::Window::new("天光设置").show(ctx, |ui| {
        ui.label("使用滑块调整方向光角度");
        ui.add(egui::Slider::new(&mut angles.azimuth_deg, 0.0..=360.0).text("方位角(°)"));
        ui.add(egui::Slider::new(&mut angles.elevation_deg, -90.0..=89.0).text("仰角(°)"));
    });
}

fn apply_sun_angles(
    angles: Res<SunAngles>,
    mut suns: Query<&mut Transform, With<DirectionalLight>>,
) {
    let Ok(mut tf) = suns.single_mut() else {
        return;
    };
    let az = angles.azimuth_deg.to_radians();
    let el = angles.elevation_deg.to_radians();
    let dir = Vec3::new(el.cos() * az.cos(), el.sin(), el.cos() * az.sin());

    let distance = 1.0;
    tf.translation = -dir * distance;
    tf.look_at(Vec3::ZERO, Vec3::Y);
}

fn setup_egui_cjk_font(mut contexts: EguiContexts) {
    let ctx = contexts.ctx_mut().expect("Failed to get context");
    let mut fonts = egui::FontDefinitions::default();

    // Embed the CJK-capable font at compile time to ensure availability in all targets (incl. WASM)
    let font_name = "cjk_font:NotoSansSC.ttf".to_string();
    let font_bytes: &'static [u8] = include_bytes!("../assets/fonts/NotoSansSC.ttf");

    fonts.font_data.insert(
        font_name.clone(),
        std::sync::Arc::new(egui::FontData::from_owned(font_bytes.to_vec())),
    );

    // Put our CJK font at the front of the fallback list for both families
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, font_name.clone());
    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .insert(0, font_name);

    ctx.set_fonts(fonts);
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
    let Ok(mut window) = window_q.single_mut() else {
        return;
    };
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    if ctx.wants_pointer_input() || ctx.wants_keyboard_input() {
        return;
    }
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
    if !mouse_buttons.pressed(MouseButton::Right) {
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    if ctx.wants_pointer_input() || ctx.wants_keyboard_input() {
        return;
    }
    let Ok(mut controller) = query.single_mut() else {
        return;
    };
    let Ok(mut transform) = xform_q.single_mut() else {
        return;
    };

    let mut delta = Vec2::ZERO;
    for ev in mouse_motion_events.read() {
        delta += ev.delta;
    }
    if delta == Vec2::ZERO {
        return;
    }

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
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    if ctx.wants_pointer_input() || ctx.wants_keyboard_input() {
        return;
    }
    let Ok(mut controller) = controller_q.single_mut() else {
        return;
    };
    let Ok(mut transform) = transform_q.single_mut() else {
        return;
    };

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
    if keys.pressed(KeyCode::KeyW) {
        input_direction += Vec3::Z;
    }
    if keys.pressed(KeyCode::KeyS) {
        input_direction += -Vec3::Z;
    }
    if keys.pressed(KeyCode::KeyA) {
        input_direction += -Vec3::X;
    }
    if keys.pressed(KeyCode::KeyD) {
        input_direction += Vec3::X;
    }
    if keys.pressed(KeyCode::KeyE) || keys.pressed(KeyCode::Space) {
        input_direction += Vec3::Y;
    }
    if keys.pressed(KeyCode::KeyQ) {
        input_direction += -Vec3::Y;
    }

    if input_direction == Vec3::ZERO {
        return;
    }

    let forward = transform.forward();
    let right = transform.right();
    let up = Vec3::Y;
    let world_dir =
        (forward * input_direction.z + right * input_direction.x + up * input_direction.y)
            .normalize();

    transform.translation += world_dir * speed * time.delta_secs();
}

// --- Heightmap import via egui ---
#[derive(Resource)]
struct HeightmapUiState {
    size_x: f32,
    size_z: f32,
    height_scale: f32,
    last_status: Option<String>,
}

impl Default for HeightmapUiState {
    fn default() -> Self {
        Self {
            size_x: 10.0,
            size_z: 10.0,
            height_scale: 10.0,
            last_status: None,
        }
    }
}

fn heightmap_ui(
    mut contexts: EguiContexts,
    mut state: ResMut<HeightmapUiState>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut terrain_q: Query<&mut Mesh3d, With<Terrain>>,
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    egui::Window::new("高度图").show(ctx, |ui| {
        ui.label("导入 RGBA PNG 高度图，仅使用 R/G 通道 (0..65535)，R=低8位，G=高8位");
        ui.add(egui::Slider::new(&mut state.size_x, 0.001..=1000.0).text("尺寸X"));
        ui.add(egui::Slider::new(&mut state.size_z, 0.001..=1000.0).text("尺寸Z"));
        ui.add(egui::Slider::new(&mut state.height_scale, 0.001..=1000.0).text("高度缩放"));

        // Poll async-loaded bytes on wasm
        #[cfg(target_arch = "wasm32")]
        if let Some(queue) = HEIGHTMAP_QUEUE.get() {
            if let Ok(mut q) = queue.lock() {
                if let Some(bytes) = q.pop() {
                    match image::load_from_memory(&bytes)
                        .ok()
                        .map(|img| img.to_rgba8())
                    {
                        Some(rgba) => {
                            let mesh = generate_mesh_from_rg_heightmap(
                                &rgba,
                                state.size_x,
                                state.size_z,
                                state.height_scale,
                            );
                            let new_handle = meshes.add(mesh);
                            if let Ok(mut mesh3d) = terrain_q.single_mut() {
                                *mesh3d = Mesh3d(new_handle);
                                state.last_status = Some(format!(
                                    "(WASM) 已载入: {}x{}，替换地形网格",
                                    rgba.width(),
                                    rgba.height()
                                ));
                            } else {
                                state.last_status = Some("未找到 Terrain 实体".to_string());
                            }
                        }
                        None => {
                            state.last_status = Some("(WASM) 解析 PNG 失败".to_string());
                        }
                    }
                }
            }
        }

        if ui.button("导入PNG高度图...").clicked() {
            #[cfg(not(target_arch = "wasm32"))]
            {
                if let Some(path) = FileDialog::new().add_filter("PNG", &["png"]).pick_file() {
                    match std::fs::read(&path)
                        .ok()
                        .and_then(|bytes| image::load_from_memory(&bytes).ok())
                        .map(|dyn_img| dyn_img.to_rgba8())
                    {
                        Some(rgba) => {
                            let mesh = generate_mesh_from_rg_heightmap(
                                &rgba,
                                state.size_x,
                                state.size_z,
                                state.height_scale,
                            );
                            let new_handle = meshes.add(mesh);
                            if let Ok(mut mesh3d) = terrain_q.single_mut() {
                                *mesh3d = Mesh3d(new_handle);
                                state.last_status = Some(format!(
                                    "已载入: {}x{}，替换地形网格",
                                    rgba.width(),
                                    rgba.height()
                                ));
                            } else {
                                state.last_status = Some("未找到 Terrain 实体".to_string());
                            }
                        }
                        None => {
                            state.last_status = Some("读取或解析 PNG 失败".to_string());
                        }
                    }
                }
            }

            #[cfg(target_arch = "wasm32")]
            {
                if let Some(queue) = HEIGHTMAP_QUEUE.get() {
                    let q = queue;
                    spawn_local(async move {
                        if let Some(file) = AsyncFileDialog::new()
                            .add_filter("PNG", &["png"])
                            .pick_file()
                            .await
                        {
                            let data = file.read().await;
                            if let Ok(mut locked) = q.lock() {
                                locked.push(data);
                            }
                        }
                    });
                    state.last_status = Some("(WASM) 已打开文件选择对话框".to_string());
                } else {
                    state.last_status = Some("(WASM) 文件队列未初始化".to_string());
                }
            }
        }

        if let Some(s) = &state.last_status {
            ui.label(s);
        }
    });
}

fn generate_mesh_from_rg_heightmap(
    img: &RgbaImage,
    size_x: f32,
    size_z: f32,
    height_scale: f32,
) -> Mesh {
    let width = img.width() as usize;
    let height = img.height() as usize;

    let vertex_count = width * height;
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(vertex_count);
    let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(vertex_count);

    let dx = if width > 1 {
        size_x / (width as f32 - 1.0)
    } else {
        0.0
    };
    let dz = if height > 1 {
        size_z / (height as f32 - 1.0)
    } else {
        0.0
    };

    let x_origin = -size_x * 0.5;
    let z_origin = -size_z * 0.5;

    for iz in 0..height {
        for ix in 0..width {
            let p = img.get_pixel(ix as u32, iz as u32);
            let r = p[0] as u16; // low 8 bits
            let g = p[1] as u16; // high 8 bits
            let h16: u16 = (r << 8) | g;
            let h = ((h16 as f32) / 65535.0 - 0.5) * height_scale;

            let x = x_origin + ix as f32 * dx;
            let z = z_origin + iz as f32 * dz;
            positions.push([x, h, z]);
            uvs.push([
                ix as f32 / (width as f32 - 1.0),
                iz as f32 / (height as f32 - 1.0),
            ]);
        }
    }

    let quad_count_x = width.saturating_sub(1);
    let quad_count_z = height.saturating_sub(1);
    let mut indices: Vec<u32> = Vec::with_capacity(quad_count_x * quad_count_z * 6);

    for iz in 0..quad_count_z {
        for ix in 0..quad_count_x {
            let i0 = (iz * width + ix) as u32;
            let i1 = i0 + 1;
            let i2 = i0 + width as u32;
            let i3 = i2 + 1;
            indices.extend_from_slice(&[i0, i2, i1, i1, i2, i3]);
        }
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(Indices::U32(indices));

    mesh.compute_normals();

    mesh
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

    let dx = if num_x > 1 {
        size_x / (num_x as f32 - 1.0)
    } else {
        0.0
    };
    let dz = if num_z > 1 {
        size_z / (num_z as f32 - 1.0)
    } else {
        0.0
    };

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

    let quad_count_x = num_x.saturating_sub(1);
    let quad_count_z = num_z.saturating_sub(1);
    let mut indices: Vec<u32> = Vec::with_capacity(quad_count_x * quad_count_z * 6);

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

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(Indices::U32(indices));

    // Compute normals for proper PBR lighting
    mesh.compute_normals();

    mesh
}
