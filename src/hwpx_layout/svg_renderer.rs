//! Connect the real paragraph/object/table/image renderers without test callbacks.
use super::{
    style_dom::ParsedXml,
    svg_images,
    svg_objects::{self, DrawLeaves},
    svg_para::{DrawObjects, ObjectArgs, RenderContext},
    svg_table,
    value::Value,
};

pub struct Renderer;

impl DrawObjects for Renderer {
    fn draw_object(
        &mut self,
        doc: &mut ParsedXml,
        args: ObjectArgs,
        ctx: &mut RenderContext,
    ) -> Result<(), String> {
        svg_objects::draw_object(doc, args, ctx, self)
    }
}

impl DrawLeaves for Renderer {
    fn draw_table(
        &mut self,
        doc: &mut ParsedXml,
        node: usize,
        x: Value,
        y: Value,
        ctx: &mut RenderContext,
        depth: usize,
    ) -> Result<(), String> {
        svg_table::draw_table(doc, node, x, y, ctx, depth, self)
    }

    fn draw_pic(
        &mut self,
        doc: &mut ParsedXml,
        node: usize,
        x: Value,
        y: Value,
        ctx: &mut RenderContext,
    ) -> Result<(), String> {
        svg_images::draw_pic(doc, node, x, y, ctx)
    }

    fn draw_pic_frame(
        &mut self,
        doc: &mut ParsedXml,
        node: usize,
        x: Value,
        y: Value,
        w: Value,
        h: Value,
        extra: &str,
        ctx: &mut RenderContext,
    ) -> Result<(), String> {
        svg_images::draw_pic_frame(doc, node, x, y, w, h, &Value::from(extra), ctx)
    }
}
