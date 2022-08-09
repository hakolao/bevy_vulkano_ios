use std::sync::Arc;

use crate::quad_pipeline::DrawQuadPipeline;
use std::convert::TryFrom;
use vulkano::{
    command_buffer::{
        AutoCommandBufferBuilder, CommandBufferInheritanceInfo, CommandBufferUsage,
        RenderPassBeginInfo, SubpassContents,
    },
    device::Queue,
    format::Format,
    image::ImageAccess,
    render_pass::{Framebuffer, FramebufferCreateInfo, RenderPass, Subpass},
    sync::GpuFuture,
};
use vulkano_util::renderer::{DeviceImageView, SwapchainImageView};

/// A render pass which places an image over screen frame
pub struct FillScreenRenderPass {
    gfx_queue: Arc<Queue>,
    render_pass: Arc<RenderPass>,
    subpass: Subpass,
    quad_pipeline: DrawQuadPipeline,
}

impl FillScreenRenderPass {
    pub fn new(gfx_queue: Arc<Queue>, output_format: Format) -> FillScreenRenderPass {
        let render_pass = vulkano::single_pass_renderpass!(gfx_queue.device().clone(),
            attachments: {
                color: {
                    load: Clear,
                    store: Store,
                    format: output_format,
                    samples: 1,
                }
            },
            pass: {
                    color: [color],
                    depth_stencil: {}
            }
        )
        .unwrap();
        let subpass = Subpass::from(render_pass.clone(), 0).unwrap();
        let quad_pipeline = DrawQuadPipeline::new(gfx_queue.clone(), subpass.clone());

        FillScreenRenderPass {
            gfx_queue,
            render_pass,
            subpass,
            quad_pipeline,
        }
    }

    /// Place view exactly over swapchain image target.
    /// Texture draw pipeline uses a quad onto which it places the view.
    pub fn draw<F>(
        &mut self,
        before_future: F,
        canvas_image: DeviceImageView,
        target: SwapchainImageView,
        clear_color: [f32; 4],
    ) -> Box<dyn GpuFuture>
    where
        F: GpuFuture + 'static,
    {
        // Get dimensions of target image
        let image_dims = target.image().dimensions().width_height();

        // Create framebuffer (must be in same order as render pass description in `new`)
        let framebuffer = Framebuffer::new(
            self.render_pass.clone(),
            FramebufferCreateInfo {
                attachments: vec![target],
                ..Default::default()
            },
        )
        .unwrap();
        // Create primary command buffer builder & begin render pass
        let mut command_buffer_builder = AutoCommandBufferBuilder::primary(
            self.gfx_queue.device().clone(),
            self.gfx_queue.family(),
            CommandBufferUsage::OneTimeSubmit,
        )
        .unwrap();
        command_buffer_builder
            .begin_render_pass(
                RenderPassBeginInfo {
                    clear_values: vec![Some(clear_color.into())],
                    ..RenderPassBeginInfo::framebuffer(framebuffer)
                },
                SubpassContents::SecondaryCommandBuffers,
            )
            .unwrap();

        // Command buffer for our single subpass
        let mut secondary_builder = AutoCommandBufferBuilder::secondary(
            self.gfx_queue.device().clone(),
            self.gfx_queue.family(),
            CommandBufferUsage::MultipleSubmit,
            CommandBufferInheritanceInfo {
                render_pass: Some(self.subpass.clone().into()),
                ..Default::default()
            },
        )
        .unwrap();

        // Draw on target
        self.quad_pipeline
            .draw(&mut secondary_builder, image_dims, canvas_image.clone());

        // Execute
        let cb = secondary_builder.build().unwrap();
        command_buffer_builder.execute_commands(cb).unwrap();

        // Finish
        command_buffer_builder.end_render_pass().unwrap();
        let command_buffer = command_buffer_builder.build().unwrap();
        before_future
            .then_execute(self.gfx_queue.clone(), command_buffer)
            .unwrap()
            .boxed()
    }
}
