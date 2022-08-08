use std::sync::Arc;

use bytemuck::Pod;
use bytemuck::Zeroable;
use vulkano::buffer::{BufferUsage, CpuAccessibleBuffer};
use vulkano::descriptor_set::{PersistentDescriptorSet, WriteDescriptorSet};
use vulkano::{
    buffer::TypedBufferAccess,
    command_buffer::{AutoCommandBufferBuilder, SecondaryAutoCommandBuffer},
    device::Queue,
    image::ImageViewAbstract,
    pipeline::{
        graphics::{
            color_blend::ColorBlendState,
            input_assembly::InputAssemblyState,
            vertex_input::BuffersDefinition,
            viewport::{Viewport, ViewportState},
        },
        GraphicsPipeline, Pipeline, PipelineBindPoint,
    },
    render_pass::Subpass,
    sampler::{Filter, Sampler, SamplerAddressMode, SamplerCreateInfo, SamplerMipmapMode},
};

/// Vertex for textured quads
#[repr(C)]
#[derive(Default, Debug, Copy, Clone, Zeroable, Pod)]
pub struct TexturedVertex {
    pub position: [f32; 2],
    pub tex_coords: [f32; 2],
}
vulkano::impl_vertex!(TexturedVertex, position, tex_coords);

pub fn textured_quad(width: f32, height: f32) -> (Vec<TexturedVertex>, Vec<u32>) {
    (
        vec![
            TexturedVertex {
                position: [-(width / 2.0), -(height / 2.0)],
                tex_coords: [0.0, 1.0],
            },
            TexturedVertex {
                position: [-(width / 2.0), height / 2.0],
                tex_coords: [0.0, 0.0],
            },
            TexturedVertex {
                position: [width / 2.0, height / 2.0],
                tex_coords: [1.0, 0.0],
            },
            TexturedVertex {
                position: [width / 2.0, -(height / 2.0)],
                tex_coords: [1.0, 1.0],
            },
        ],
        vec![0, 2, 1, 0, 3, 2],
    )
}

fn create_sampler_decriptor_set(
    pipeline: Arc<GraphicsPipeline>,
    sampler: Arc<Sampler>,
    image: Arc<dyn ImageViewAbstract>,
) -> Arc<PersistentDescriptorSet> {
    let layout = pipeline.layout().set_layouts().get(0).unwrap();
    PersistentDescriptorSet::new(
        layout.clone(),
        [WriteDescriptorSet::image_view_sampler(
            0,
            image.clone(),
            sampler,
        )],
    )
    .unwrap()
}

/// Pipeline to draw pixel perfect images on quads
pub struct DrawQuadPipeline {
    pipeline: Arc<GraphicsPipeline>,
    sampler: Arc<Sampler>,
    vertices: Arc<CpuAccessibleBuffer<[TexturedVertex]>>,
    indices: Arc<CpuAccessibleBuffer<[u32]>>,
}

impl DrawQuadPipeline {
    pub fn new(gfx_queue: Arc<Queue>, subpass: Subpass) -> DrawQuadPipeline {
        let (vertices, indices) = textured_quad(2.0, 2.0);
        let vertex_buffer = CpuAccessibleBuffer::<[TexturedVertex]>::from_iter(
            gfx_queue.device().clone(),
            BufferUsage::vertex_buffer(),
            false,
            vertices.into_iter(),
        )
        .unwrap();
        let index_buffer = CpuAccessibleBuffer::<[u32]>::from_iter(
            gfx_queue.device().clone(),
            BufferUsage::index_buffer(),
            false,
            indices.into_iter(),
        )
        .unwrap();

        let pipeline = {
            let vs = vs::load(gfx_queue.device().clone()).expect("failed to create shader module");
            let fs = fs::load(gfx_queue.device().clone()).expect("failed to create shader module");
            GraphicsPipeline::start()
                .vertex_input_state(BuffersDefinition::new().vertex::<TexturedVertex>())
                .vertex_shader(vs.entry_point("main").unwrap(), ())
                .input_assembly_state(InputAssemblyState::new())
                .fragment_shader(fs.entry_point("main").unwrap(), ())
                .viewport_state(ViewportState::viewport_dynamic_scissor_irrelevant())
                .render_pass(subpass.clone())
                .color_blend_state(ColorBlendState::default().blend_alpha())
                .build(gfx_queue.device().clone())
                .unwrap()
        };
        let sampler = Sampler::new(
            gfx_queue.device().clone(),
            SamplerCreateInfo {
                mag_filter: Filter::Nearest,
                min_filter: Filter::Nearest,
                address_mode: [SamplerAddressMode::ClampToEdge; 3],
                mipmap_mode: SamplerMipmapMode::Nearest,
                ..Default::default()
            },
        )
        .unwrap();
        DrawQuadPipeline {
            pipeline,
            sampler,
            vertices: vertex_buffer,
            indices: index_buffer,
        }
    }

    pub fn draw(
        &mut self,
        builder: &mut AutoCommandBufferBuilder<SecondaryAutoCommandBuffer>,
        viewport_dimensions: [u32; 2],
        image: Arc<dyn ImageViewAbstract>,
    ) {
        let push_constants = vs::ty::PushConstants {
            world_to_screen: bevy::math::Mat4::IDENTITY.to_cols_array_2d(),
        };
        let image_sampler_descriptor_set =
            create_sampler_decriptor_set(self.pipeline.clone(), self.sampler.clone(), image);
        builder
            .set_viewport(
                0,
                [Viewport {
                    origin: [0.0, 0.0],
                    dimensions: [viewport_dimensions[0] as f32, viewport_dimensions[1] as f32],
                    depth_range: 0.0..1.0,
                }],
            )
            .bind_pipeline_graphics(self.pipeline.clone())
            .bind_descriptor_sets(
                PipelineBindPoint::Graphics,
                self.pipeline.layout().clone(),
                0,
                image_sampler_descriptor_set,
            )
            .push_constants(self.pipeline.layout().clone(), 0, push_constants)
            .bind_vertex_buffers(0, self.vertices.clone())
            .bind_index_buffer(self.indices.clone())
            .draw_indexed(self.indices.len() as u32, 1, 0, 0, 0)
            .unwrap();
    }
}

mod vs {
    vulkano_shaders::shader! {
        ty: "vertex",
        src: "
#version 450
layout(location=0) in vec2 position;
layout(location=1) in vec2 tex_coords;

layout(push_constant) uniform PushConstants {
    mat4 world_to_screen;
} push_constants;

layout(location = 0) out vec2 f_tex_coords;

void main() {
    gl_Position =  push_constants.world_to_screen * vec4(position, 0.0, 1.0);
    f_tex_coords = tex_coords;
}
        "
    }
}

mod fs {
    vulkano_shaders::shader! {
        ty: "fragment",
        src: "
#version 450
layout(location = 0) in vec2 v_tex_coords;

layout(location = 0) out vec4 f_color;

layout(set = 0, binding = 0) uniform sampler2D tex;

void main() {
    f_color = texture(tex, v_tex_coords);
}
"
    }
}
