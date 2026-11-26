//! Bloom: bright things on the plot bleed light into what is around them.
//!
//! egui draws the map straight to the window, so this runs as a paint callback placed
//! after the map's symbols and before its labels. It copies what has been drawn so far,
//! keeps only the bright part at quarter size, blurs that, and adds it back. Nothing
//! drawn after the callback (labels, readouts, the command deck) is touched.
//!
//! It needs the OpenGL renderer. On wgpu, in tests, or if any GL step fails, no callback
//! is added and the plot is drawn exactly as before; the gradient glows in `fx` do not
//! depend on it.
use super::*;
use eframe::glow::{self, HasContext as _};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Set once at startup when the OpenGL renderer is the one running.
pub(super) static GL_RENDERER: AtomicBool = AtomicBool::new(false);

/// Luminance at which a pixel starts to bloom is `THRESHOLD - KNEE`. Only the excess above it
/// spills, rising smoothly, so a broad bright surface like the star's disc gains a halo
/// without washing out to white.
const THRESHOLD: f32 = 0.66;
const KNEE: f32 = 0.16;
/// How much of the blurred light is added back.
const INTENSITY: f32 = 0.62;
/// The blur runs at one over this of the plot's resolution.
const DIVISOR: i32 = 4;
const PASSES: usize = 2;

const VERTEX: &str = "out vec2 uv;\nvoid main() {\n  vec2 p = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));\n  uv = p;\n  gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);\n}\n";
/// Keep the bright part, averaging four taps so a one-pixel star does not flicker as it is shrunk.
const BRIGHT: &str = "uniform sampler2D source;\nuniform vec2 texel;\nuniform float threshold;\nuniform float knee;\nin vec2 uv;\nout vec4 color;\nvoid main() {\n  vec3 sum = vec3(0.0);\n  for (int i = 0; i < 4; i++) {\n    vec2 at = uv + texel * vec2(float(i & 1) * 2.0 - 1.0, float(i >> 1) * 2.0 - 1.0);\n    vec3 c = texture(source, at).rgb;\n    float l = dot(c, vec3(0.2126, 0.7152, 0.0722));\n    float over = clamp((l - threshold + knee) / (1.0 - threshold + knee), 0.0, 1.0);\n    sum += c * over * sqrt(over);\n  }\n  color = vec4(sum * 0.25, 1.0);\n}\n";
const BLUR: &str = "uniform sampler2D source;\nuniform vec2 step;\nin vec2 uv;\nout vec4 color;\nvoid main() {\n  vec3 sum = texture(source, uv).rgb * 0.2270270270;\n  sum += (texture(source, uv + step * 1.3846153846).rgb + texture(source, uv - step * 1.3846153846).rgb) * 0.3162162162;\n  sum += (texture(source, uv + step * 3.2307692308).rgb + texture(source, uv - step * 3.2307692308).rgb) * 0.0702702703;\n  color = vec4(sum, 1.0);\n}\n";
const COMPOSITE: &str = "uniform sampler2D source;\nuniform float intensity;\nin vec2 uv;\nout vec4 color;\nvoid main() {\n  color = vec4(texture(source, uv).rgb * intensity, 0.0);\n}\n";

struct Target {
    texture: glow::Texture,
    framebuffer: glow::Framebuffer,
}

struct Resources {
    bright: glow::Program,
    blur: glow::Program,
    composite: glow::Program,
    vertex_array: glow::VertexArray,
    scene: glow::Texture,
    targets: [Target; 2],
    /// Plot size in pixels the textures were made for.
    size: (i32, i32),
}

enum State {
    Untried,
    Ready(Resources),
    Failed,
}

#[derive(Clone)]
pub(super) struct Bloom {
    state: Arc<Mutex<State>>,
    pub enabled: bool,
}

impl Default for Bloom {
    fn default() -> Self {
        let wanted = std::env::var("LUMINAL_BLOOM").map_or(true, |v| v != "0");
        Self { state: Arc::new(Mutex::new(State::Untried)), enabled: wanted }
    }
}

impl Bloom {
    /// Whether this build and renderer can bloom at all.
    pub(super) fn available(&self) -> bool {
        GL_RENDERER.load(Ordering::Relaxed) && !matches!(*self.state.lock().unwrap(), State::Failed)
    }

    /// Bloom everything drawn on `rect` so far. Call after the plot's symbols and before its labels.
    pub(super) fn paint(&self, painter: &egui::Painter, rect: Rect) {
        if !self.enabled || !self.available() || !rect.is_positive() { return; }
        let state = self.state.clone();
        painter.add(egui::PaintCallback {
            rect,
            callback: Arc::new(eframe::egui_glow::CallbackFn::new(move |info, painter| {
                let mut state = state.lock().unwrap();
                if matches!(*state, State::Failed) { return; }
                let gl = painter.gl();
                let viewport = info.viewport_in_pixels();
                let clip = info.clip_rect_in_pixels();
                let area = (viewport.left_px, viewport.from_bottom_px, viewport.width_px, viewport.height_px);
                let scissor = (clip.left_px, clip.from_bottom_px, clip.width_px, clip.height_px);
                // SAFETY: called by egui_glow on the thread that owns the context, with that context current.
                if let Err(error) = unsafe { run(gl, &mut state, area, scissor) } {
                    eprintln!("Bloom disabled: {error}");
                    *state = State::Failed;
                }
            })),
        });
    }
}

unsafe fn program(gl: &glow::Context, header: &str, fragment: &str) -> Result<glow::Program, String> {
    unsafe {
        let program = gl.create_program()?;
        let mut shaders = vec![];
        for (kind, source) in [(glow::VERTEX_SHADER, VERTEX), (glow::FRAGMENT_SHADER, fragment)] {
            let shader = gl.create_shader(kind)?;
            gl.shader_source(shader, &format!("{header}{source}"));
            gl.compile_shader(shader);
            if !gl.get_shader_compile_status(shader) { return Err(gl.get_shader_info_log(shader)); }
            gl.attach_shader(program, shader);
            shaders.push(shader);
        }
        gl.link_program(program);
        if !gl.get_program_link_status(program) { return Err(gl.get_program_info_log(program)); }
        for shader in shaders { gl.detach_shader(program, shader); gl.delete_shader(shader); }
        Ok(program)
    }
}

unsafe fn texture(gl: &glow::Context, width: i32, height: i32) -> Result<glow::Texture, String> {
    unsafe {
        let texture = gl.create_texture()?;
        gl.bind_texture(glow::TEXTURE_2D, Some(texture));
        gl.tex_image_2d(glow::TEXTURE_2D, 0, glow::RGBA8 as i32, width, height, 0, glow::RGBA, glow::UNSIGNED_BYTE, glow::PixelUnpackData::Slice(None));
        for (name, value) in [(glow::TEXTURE_MIN_FILTER, glow::LINEAR), (glow::TEXTURE_MAG_FILTER, glow::LINEAR), (glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE), (glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE)] {
            gl.tex_parameter_i32(glow::TEXTURE_2D, name, value as i32);
        }
        Ok(texture)
    }
}

unsafe fn build(gl: &glow::Context, size: (i32, i32), old: Option<Resources>) -> Result<Resources, String> {
    unsafe {
        let (bright, blur, composite, vertex_array) = match old {
            Some(old) => {
                gl.delete_texture(old.scene);
                for target in &old.targets { gl.delete_texture(target.texture); gl.delete_framebuffer(target.framebuffer); }
                (old.bright, old.blur, old.composite, old.vertex_array)
            }
            None => {
                let version = eframe::egui_glow::ShaderVersion::get(gl);
                if !version.is_new_shader_interface() { return Err("the OpenGL version is too old for the bloom shaders".into()); }
                let header = format!("{}{}", version.version_declaration(), if version.is_embedded() { "precision mediump float;\n" } else { "" });
                (program(gl, &header, BRIGHT)?, program(gl, &header, BLUR)?, program(gl, &header, COMPOSITE)?, gl.create_vertex_array()?)
            }
        };
        let scene = texture(gl, size.0, size.1)?;
        let small = ((size.0 / DIVISOR).max(1), (size.1 / DIVISOR).max(1));
        let mut targets = vec![];
        for _ in 0..2 {
            let texture = texture(gl, small.0, small.1)?;
            let framebuffer = gl.create_framebuffer()?;
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(framebuffer));
            gl.framebuffer_texture_2d(glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT0, glow::TEXTURE_2D, Some(texture), 0);
            if gl.check_framebuffer_status(glow::FRAMEBUFFER) != glow::FRAMEBUFFER_COMPLETE { return Err("the bloom framebuffer is incomplete".into()); }
            targets.push(Target { texture, framebuffer });
        }
        let [a, b]: [Target; 2] = targets.try_into().map_err(|_| "two bloom targets".to_string())?;
        Ok(Resources { bright, blur, composite, vertex_array, scene, targets: [a, b], size })
    }
}

/// One bloom of the plot. `area` and `scissor` are `(left, bottom, width, height)` in pixels.
unsafe fn run(gl: &glow::Context, state: &mut State, area: (i32, i32, i32, i32), scissor: (i32, i32, i32, i32)) -> Result<(), String> {
    unsafe {
        let (left, bottom, width, height) = area;
        if width < 8 || height < 8 { return Ok(()); }
        // Wherever egui is drawing to is where the light goes back.
        let target = gl.get_parameter_framebuffer(glow::FRAMEBUFFER_BINDING);
        let current = std::mem::replace(state, State::Failed);
        let resources = match current {
            State::Ready(resources) if resources.size == (width, height) => resources,
            State::Ready(resources) => build(gl, (width, height), Some(resources))?,
            State::Untried => build(gl, (width, height), None)?,
            State::Failed => return Ok(()),
        };
        gl.bind_framebuffer(glow::FRAMEBUFFER, target);
        gl.active_texture(glow::TEXTURE0);
        gl.bind_texture(glow::TEXTURE_2D, Some(resources.scene));
        gl.copy_tex_sub_image_2d(glow::TEXTURE_2D, 0, 0, 0, left, bottom, width, height);

        let small = ((width / DIVISOR).max(1), (height / DIVISOR).max(1));
        gl.disable(glow::SCISSOR_TEST);
        gl.disable(glow::BLEND);
        gl.bind_vertex_array(Some(resources.vertex_array));
        gl.viewport(0, 0, small.0, small.1);

        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(resources.targets[0].framebuffer));
        gl.use_program(Some(resources.bright));
        gl.uniform_1_i32(gl.get_uniform_location(resources.bright, "source").as_ref(), 0);
        gl.uniform_2_f32(gl.get_uniform_location(resources.bright, "texel").as_ref(), 1.0 / width as f32, 1.0 / height as f32);
        gl.uniform_1_f32(gl.get_uniform_location(resources.bright, "threshold").as_ref(), THRESHOLD);
        gl.uniform_1_f32(gl.get_uniform_location(resources.bright, "knee").as_ref(), KNEE);
        gl.draw_arrays(glow::TRIANGLES, 0, 3);

        gl.use_program(Some(resources.blur));
        gl.uniform_1_i32(gl.get_uniform_location(resources.blur, "source").as_ref(), 0);
        let step = gl.get_uniform_location(resources.blur, "step");
        for pass in 0..PASSES {
            let reach = 1.0 + pass as f32;
            for (from, to, across) in [(0, 1, (reach / small.0 as f32, 0.0)), (1, 0, (0.0, reach / small.1 as f32))] {
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(resources.targets[to].framebuffer));
                gl.bind_texture(glow::TEXTURE_2D, Some(resources.targets[from].texture));
                gl.uniform_2_f32(step.as_ref(), across.0, across.1);
                gl.draw_arrays(glow::TRIANGLES, 0, 3);
            }
        }

        // Add the blurred light back over the plot, and nowhere else.
        gl.bind_framebuffer(glow::FRAMEBUFFER, target);
        gl.viewport(left, bottom, width, height);
        gl.enable(glow::SCISSOR_TEST);
        gl.scissor(scissor.0, scissor.1, scissor.2, scissor.3);
        gl.enable(glow::BLEND);
        gl.blend_func(glow::ONE, glow::ONE);
        gl.use_program(Some(resources.composite));
        gl.uniform_1_i32(gl.get_uniform_location(resources.composite, "source").as_ref(), 0);
        gl.uniform_1_f32(gl.get_uniform_location(resources.composite, "intensity").as_ref(), INTENSITY);
        gl.bind_texture(glow::TEXTURE_2D, Some(resources.targets[0].texture));
        gl.draw_arrays(glow::TRIANGLES, 0, 3);
        gl.bind_texture(glow::TEXTURE_2D, None);
        gl.bind_vertex_array(None);
        // egui_glow restores its own blend, program, viewport and vertex array after a callback.
        *state = State::Ready(resources);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_the_opengl_renderer_nothing_is_added_to_the_plot() {
        let bloom = Bloom::default();
        assert!(!bloom.available(), "tests run headless");
        let ctx = egui::Context::default();
        let rect = Rect::from_min_size(Pos2::ZERO, EVec2::new(400.0, 300.0));
        let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(rect), ..Default::default() }, |ui| bloom.paint(&ui.painter_at(rect), rect));
        output.textures_delta.clear();
        assert!(!output.shapes.iter().any(|s| matches!(s.shape, Shape::Callback(_))));
    }

    #[test]
    fn the_shaders_keep_only_bright_light_and_add_it_without_covering() {
        // The pass is additive and thresholded: these are the properties that keep labels and dim symbols untouched.
        assert!(COMPOSITE.contains("* intensity, 0.0)"), "alpha zero, so the blend can only add");
        assert!(BRIGHT.contains("clamp((l - threshold + knee) / (1.0 - threshold + knee), 0.0, 1.0)"));
        let (starts, hostile_red) = (THRESHOLD - KNEE, 0.46);
        assert!(starts > hostile_red, "a hostile red at luminance 0.46 stays below the knee");
        let weights = 0.2270270270 + 2.0 * (0.3162162162 + 0.0702702703);
        assert!((weights - 1.0_f64).abs() < 1e-6, "the blur neither brightens nor dims");
    }
}
