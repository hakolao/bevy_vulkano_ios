mod game_of_life;
mod quad_pipeline;
mod render_pass;

use crate::game_of_life::GameOfLife;
use crate::render_pass::FillScreenRenderPass;
use bevy::input::touch::touch_screen_input_system;
use bevy::prelude::*;
use bevy::time::FixedTimestep;
use bevy::window::{WindowDescriptor, WindowResized};
use bevy_vulkano::{BevyVulkanoWindows, VulkanoWinitConfig, VulkanoWinitPlugin};
use mobile_entry_point::mobile_entry_point;
use vulkano::image::ImageAccess;

const WIDTH: u32 = 128;
const HEIGHT: u32 = 256;
const CLEAR_COLOR: [f32; 4] = [0.0; 4];

#[mobile_entry_point]
fn main() {
    App::new()
        .insert_non_send_resource(VulkanoWinitConfig::default())
        .insert_resource(WindowDescriptor {
            width: (WIDTH * 2) as f32,
            height: (HEIGHT * 2) as f32,
            ..WindowDescriptor::default()
        })
        .add_plugin(bevy::core::CorePlugin)
        .add_plugin(bevy::input::InputPlugin)
        .add_plugin(bevy::time::TimePlugin)
        .add_plugin(VulkanoWinitPlugin)
        .add_startup_system(startup)
        .add_system(touch_screen_input_system)
        .add_system(draw_life_system)
        .add_system_set_to_stage(
            CoreStage::Update,
            SystemSet::new()
                .with_run_criteria(FixedTimestep::steps_per_second(30.0))
                .with_system(simulate.after(draw_life_system)),
        )
        .add_system_set_to_stage(CoreStage::PostUpdate, SystemSet::new().with_system(render))
        .add_system_set_to_stage(
            CoreStage::Last,
            SystemSet::new().with_system(update_image_size_on_resize),
        )
        .run();
}

fn startup(mut commands: Commands, vulkano_windows: NonSend<BevyVulkanoWindows>) {
    let primary_window = vulkano_windows.get_primary_window_renderer().unwrap();
    // Create compute pipeline to simulate game of life
    let game_of_life = GameOfLife::new(primary_window.graphics_queue(), [WIDTH, HEIGHT]);

    // Create our render pass
    let fill_screen = FillScreenRenderPass::new(
        primary_window.graphics_queue(),
        primary_window.swapchain_format(),
    );
    // Insert resources
    commands.insert_resource(game_of_life);
    commands.insert_resource(fill_screen);
}

// Ensure image size is good for the resolution
fn update_image_size_on_resize(
    mut commands: Commands,
    vulkano_windows: NonSend<BevyVulkanoWindows>,
    mut event_reader: EventReader<WindowResized>,
) {
    if let Some(e) = event_reader.iter().last() {
        let primary = vulkano_windows.get_primary_window_renderer().unwrap();
        let scale = 2;
        // Shader local sizes are 8
        let width = e.width as u32 / scale - ((e.width as u32 / scale) % 8);
        let height = e.height as u32 / scale - ((e.height as u32 / scale) % 8);
        let game_of_life = GameOfLife::new(primary.graphics_queue(), [width, height]);
        commands.insert_resource(game_of_life);
    }
}

/// Draw life at mouse position on the game of life canvas
fn draw_life_system(
    mut game_of_life: ResMut<GameOfLife>,
    windows: ResMut<Windows>,
    mouse_input: Res<Input<MouseButton>>,
    #[cfg(target_os = "ios")] touches: Res<Touches>,
) {
    fn normalized_window_pos(pos: Vec2, window: &bevy::window::Window) -> Vec2 {
        let width = window.width();
        let height = window.height();
        Vec2::new(
            (pos.x / width).clamp(0.0, 1.0),
            (pos.y / height).clamp(0.0, 1.0),
        )
    }
    if mouse_input.pressed(MouseButton::Left) {
        let primary = windows.get_primary().unwrap();
        if let Some(pos) = primary.cursor_position() {
            let normalized = normalized_window_pos(pos, &primary);
            let image_size = game_of_life
                .color_image()
                .image()
                .dimensions()
                .width_height();
            let draw_pos = IVec2::new(
                (image_size[0] as f32 * normalized.x) as i32,
                (image_size[1] as f32 * normalized.y) as i32,
            );
            game_of_life.draw_life(draw_pos, 6);
        }
    }
    #[cfg(target_os = "ios")]
    for touch in touches.iter() {
        let pos = touch.position();
        let normalized = normalized_window_pos(pos, &windows.get_primary().unwrap());
        let image_size = game_of_life
            .color_image()
            .image()
            .dimensions()
            .width_height();
        let draw_pos = IVec2::new(
            (image_size[0] as f32 * normalized.x) as i32,
            (image_size[1] as f32 * normalized.y) as i32,
        );
        game_of_life.draw_life(draw_pos, 6);
    }
}

fn simulate(mut game_of_life: ResMut<GameOfLife>) {
    game_of_life.compute([1.0, 0.0, 0.0, 1.0], [0.0; 4]);
}

/// All render occurs here in one system. If you want to split systems to separate, use
/// `PipelineSyncData` to update futures. You could have `pre_render_system` and `post_render_system` to start and finish frames
fn render(
    mut vulkano_windows: NonSendMut<BevyVulkanoWindows>,
    game_of_life: Res<GameOfLife>,
    mut fill_screen: ResMut<FillScreenRenderPass>,
) {
    let primary_window = vulkano_windows.get_primary_window_renderer_mut().unwrap();

    // Start frame
    let before = match primary_window.acquire() {
        Err(e) => {
            bevy::log::error!("Failed to start frame: {}", e);
            return;
        }
        Ok(f) => f,
    };

    let color_image = game_of_life.color_image();
    let final_image = primary_window.swapchain_image_view();
    let after_render = fill_screen.draw(before, color_image, final_image, CLEAR_COLOR);

    // Finish Frame
    primary_window.present(after_render, true);
}
