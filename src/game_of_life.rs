// Copyright (c) 2022 The vulkano developers
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>,
// at your option. All files in the project carrying such
// notice may not be copied, modified, or distributed except
// according to those terms.

use std::sync::Arc;

use bevy::math::IVec2;
use rand::Rng;
use vulkano::command_buffer::PrimaryCommandBuffer;
use vulkano::{
    buffer::{BufferUsage, CpuAccessibleBuffer},
    command_buffer::{AutoCommandBufferBuilder, CommandBufferUsage, PrimaryAutoCommandBuffer},
    descriptor_set::{PersistentDescriptorSet, WriteDescriptorSet},
    device::Queue,
    format::Format,
    image::{ImageAccess, ImageUsage, StorageImage},
    pipeline::{ComputePipeline, Pipeline, PipelineBindPoint},
    sync::GpuFuture,
};
use vulkano_util::renderer::DeviceImageView;

/// Pipeline holding double buffered grid & color image.
/// Grids are used to calculate the state, and color image is used to show the output.
/// Because each step we determine state in parallel, we need to write the output to
/// another grid. Otherwise the state would not be correctly determined as one thread might read
/// data that was just written by another thread
pub struct GameOfLife {
    compute_queue: Arc<Queue>,
    compute_life_pipeline: Arc<ComputePipeline>,
    life_in: Arc<CpuAccessibleBuffer<[u32]>>,
    life_out: Arc<CpuAccessibleBuffer<[u32]>>,
    image: DeviceImageView,
    sim_steps: u32,
}

fn rand_grid(compute_queue: &Arc<Queue>, size: [u32; 2]) -> Arc<CpuAccessibleBuffer<[u32]>> {
    CpuAccessibleBuffer::from_iter(
        compute_queue.device().clone(),
        BufferUsage::all(),
        false,
        (0..(size[0] * size[1]))
            .map(|_| rand::thread_rng().gen_range(0u32..=1))
            .collect::<Vec<u32>>(),
    )
    .unwrap()
}

impl GameOfLife {
    pub fn new(compute_queue: Arc<Queue>, size: [u32; 2]) -> GameOfLife {
        let life_in = rand_grid(&compute_queue, size);
        let life_out = rand_grid(&compute_queue, size);

        let compute_life_pipeline = {
            let shader = compute_life_cs::load(compute_queue.device().clone()).unwrap();
            ComputePipeline::new(
                compute_queue.device().clone(),
                shader.entry_point("main").unwrap(),
                &(),
                None,
                |_| {},
            )
            .unwrap()
        };

        let image = StorageImage::general_purpose_image_view(
            compute_queue.clone(),
            size,
            Format::R8G8B8A8_UNORM,
            ImageUsage {
                sampled: true,
                storage: true,
                transfer_dst: true,
                ..ImageUsage::none()
            },
        )
        .unwrap();
        GameOfLife {
            compute_queue,
            compute_life_pipeline,
            life_in,
            life_out,
            image,
            sim_steps: 0,
        }
    }

    pub fn color_image(&self) -> DeviceImageView {
        self.image.clone()
    }

    pub fn draw_life(&mut self, pos: IVec2, radius: i32) {
        let mut life_in = {
            if self.sim_steps % 2 == 0 {
                self.life_out.write().unwrap()
            } else {
                self.life_in.write().unwrap()
            }
        };
        let size = self.image.image().dimensions().width_height();
        if pos.y < 0 || pos.y >= size[1] as i32 || pos.x < 0 || pos.x >= size[0] as i32 {
            return;
        }
        let y_start = pos.y - radius;
        let y_end = pos.y + radius;
        let x_start = pos.x - radius;
        let x_end = pos.x + radius;
        for y in y_start..=y_end {
            for x in x_start..=x_end {
                let world_pos = bevy::math::Vec2::new(x as f32, y as f32);
                if world_pos
                    .distance(bevy::math::Vec2::new(pos.x as f32, pos.y as f32))
                    .round()
                    <= radius as f32
                {
                    let pos = world_pos.as_ivec2();
                    if pos.y < 0 || pos.y >= size[1] as i32 || pos.x < 0 || pos.x >= size[0] as i32
                    {
                        continue;
                    }
                    let index = (pos.y * size[0] as i32 + pos.x) as usize;
                    if rand::thread_rng().gen::<f32>() > 0.5 {
                        life_in[index] = 1
                    };
                }
            }
        }
    }

    pub fn compute(&mut self, life_color: [f32; 4], dead_color: [f32; 4]) {
        let mut builder = AutoCommandBufferBuilder::primary(
            self.compute_queue.device().clone(),
            self.compute_queue.family(),
            CommandBufferUsage::OneTimeSubmit,
        )
        .unwrap();

        // Dispatch will mutate the builder adding commands which won't be sent before we build the command buffer
        // after dispatches. This will minimize the commands we send to the GPU. For example, we could be doing
        // tens of dispatches here depending on our needs. Maybe we wanted to simulate 10 steps at a time...

        // First compute the next state. Swap buffers
        self.dispatch(
            &mut builder,
            life_color,
            dead_color,
            0,
            self.sim_steps % 2 == 0,
        );

        // Then color based on the next state. Don't swap buffers
        self.dispatch(&mut builder, life_color, dead_color, 1, false);

        let command_buffer = builder.build().unwrap();

        let finished = command_buffer.execute(self.compute_queue.clone()).unwrap();
        let _ = finished
            .then_signal_fence_and_flush()
            .unwrap()
            .wait(None)
            .unwrap();
        self.sim_steps += 1;
    }

    /// Build the command for a dispatch.
    fn dispatch(
        &mut self,
        builder: &mut AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
        life_color: [f32; 4],
        dead_color: [f32; 4],
        // Step determines whether we color or compute life (see branch in the shader)s
        step: i32,
        swap_read_order: bool,
    ) {
        // Resize image if needed
        let img_dims = self.image.image().dimensions().width_height();
        let pipeline_layout = self.compute_life_pipeline.layout();
        let desc_layout = pipeline_layout.set_layouts().get(0).unwrap();
        let set = PersistentDescriptorSet::new(
            desc_layout.clone(),
            [
                WriteDescriptorSet::image_view(0, self.image.clone()),
                WriteDescriptorSet::buffer(1, self.life_in.clone()),
                WriteDescriptorSet::buffer(2, self.life_out.clone()),
            ],
        )
        .unwrap();

        let push_constants = compute_life_cs::ty::PushConstants {
            life_color,
            dead_color,
            step,
            swap_read_order: swap_read_order as u32,
        };
        builder
            .bind_pipeline_compute(self.compute_life_pipeline.clone())
            .bind_descriptor_sets(PipelineBindPoint::Compute, pipeline_layout.clone(), 0, set)
            .push_constants(pipeline_layout.clone(), 0, push_constants)
            .dispatch([img_dims[0] / 8, img_dims[1] / 8, 1])
            .unwrap();
    }
}

mod compute_life_cs {
    vulkano_shaders::shader! {
        ty: "compute",
        src: "
#version 450

layout(local_size_x = 8, local_size_y = 8, local_size_z = 1) in;

layout(set = 0, binding = 0, rgba8) uniform writeonly image2D img;
layout(set = 0, binding = 1) buffer LifeInBuffer { uint life_in[]; };
layout(set = 0, binding = 2) buffer LifeOutBuffer { uint life_out[]; };

layout(push_constant) uniform PushConstants {
    vec4 life_color;
    vec4 dead_color;
    int step;
    bool swap_read_order;
} push_constants;

int get_index(ivec2 pos) {
    ivec2 dims = ivec2(imageSize(img));
    return pos.y * dims.x + pos.x;
}

// On iOS it seems that std::mem::swap for buffers causes
// GPU Address Fault Error (0000000b:kIOGPUCommandBufferCallbackErrorPageFault)
// Thus I'll just read and write depending on whether swap is needed

void write_life(uint index, uint life) {
    if (push_constants.swap_read_order) {
        life_in[index] = life;
    } else {
        life_out[index] = life;
    }
}

uint read_life(uint index) {
    if (push_constants.swap_read_order) {
        return life_out[index];
    } else {
        return life_in[index];
    }
}

// https://en.wikipedia.org/wiki/Conway%27s_Game_of_Life
void compute_life() {
    ivec2 pos = ivec2(gl_GlobalInvocationID.xy);
    int index = get_index(pos);
    
    ivec2 up_left = pos + ivec2(-1, 1);
    ivec2 up = pos + ivec2(0, 1);
    ivec2 up_right = pos + ivec2(1, 1);
    ivec2 right = pos + ivec2(1, 0);
    ivec2 down_right = pos + ivec2(1, -1);
    ivec2 down = pos + ivec2(0, -1);
    ivec2 down_left = pos + ivec2(-1, -1);
    ivec2 left = pos + ivec2(-1, 0);

    int alive_count = 0;
    if (life_out[get_index(up_left)] == 1) { alive_count += 1; }
    if (life_out[get_index(up)] == 1) { alive_count += 1; }
    if (life_out[get_index(up_right)] == 1) { alive_count += 1; }
    if (life_out[get_index(right)] == 1) { alive_count += 1; }
    if (life_out[get_index(down_right)] == 1) { alive_count += 1; }
    if (life_out[get_index(down)] == 1) { alive_count += 1; }
    if (life_out[get_index(down_left)] == 1) { alive_count += 1; }
    if (life_out[get_index(left)] == 1) { alive_count += 1; }

    uint current_life = read_life(index);
    // Dead becomes alive
    if (current_life == 0 && alive_count == 3) {
        write_life(index, 1);
    } // Becomes dead
    else if (current_life == 1 && alive_count < 2 || alive_count > 3) {
        write_life(index, 0);
    } // Else Do nothing
    else {
        write_life(index, current_life);
    }
}

void compute_color() {
    ivec2 pos = ivec2(gl_GlobalInvocationID.xy);
    int index = get_index(pos);
    if (life_out[index] == 1) {
        imageStore(img, pos, push_constants.life_color);
    } else {
        imageStore(img, pos, push_constants.dead_color);
    }
}

void main() {
    if (push_constants.step == 0) {
        compute_life();
    } else {
        compute_color();
    }
}"
    }
}
