//! Spumcake's renderer: it draws a place's scene file the way its pre-vis builder drew it, from a named camera
//! or a free one, in a look, at a size and a time in the scene's animation, on wgpu.
//!
//! A builder fills each face with Pillow's own scanline rule and settles overlaps in drawing order, so the
//! engine does the same: the CPU projects and fills every face to runs of pixels exactly as the builder does
//! ([`fill`]), and the GPU settles, for every pixel, which face is nearest and where the edges fall. The result
//! is the colour image, the depth in metres, and which object each pixel shows.
//!
//! [`Renderer`] draws on a device of its own and reads each frame back, for the command line and tests. [`Live`]
//! draws on a window's device and keeps the frame there, for the window's own render pass.

pub mod fill;
mod gpu;
mod prepare;

pub use gpu::{Live, Renderer, device, device_on};
pub use scene;
pub use wgpu;

/// How the scene is drawn, from the scene file's `looks`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    /// A flat grey for each kind, shaded by the light.
    Grey,
    /// A flat colour for each kind, shaded by the light.
    Coded,
    /// White faces with no shading, and only the objects the look keeps.
    Simple,
}

impl Look {
    pub fn parse(text: &str) -> Option<Look> {
        match text {
            "grey" => Some(Look::Grey),
            "coded" => Some(Look::Coded),
            "simple" => Some(Look::Simple),
            _ => None,
        }
    }
}

/// Where the frame is seen from.
#[derive(Clone, Copy, Debug)]
pub enum View<'a> {
    /// One of the scene's cameras, by name.
    Named(&'a str),
    /// Any camera: a position, a point it looks at, and a horizontal field of view in degrees, with z up.
    Free { position: [f64; 3], target: [f64; 3], hfov: f64 },
}

/// One frame to draw.
#[derive(Clone, Copy, Debug)]
pub struct Request<'a> {
    pub camera: View<'a>,
    pub look: Look,
    pub width: u32,
    pub height: u32,
    /// Seconds into the scene's animation.
    pub time: f64,
}

/// A drawn frame, row by row from the top left.
#[derive(Clone, Debug)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    /// Red, green, and blue for each pixel.
    pub colour: Vec<u8>,
    /// The distance along the camera's forward axis to what each pixel shows, in metres, or infinity.
    pub depth: Vec<f32>,
    /// Which of the scene's objects each pixel shows, as an index into `Scene::objects`, or [`Frame::NONE`].
    pub objects: Vec<u32>,
}

impl Frame {
    /// The object pass's value where no object is drawn.
    pub const NONE: u32 = u32::MAX;
}

#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// The scene has no camera by this name.
    Camera(String),
    /// The look gives no grey or colour for this kind.
    Kind(String),
    /// The scene file has no simple look.
    NoSimpleLook,
    /// A frame must be at least one pixel each way.
    Size(u32, u32),
    /// The GPU could not be reached, or failed.
    Gpu(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Camera(name) => write!(f, "the scene has no camera called \"{name}\""),
            Error::Kind(kind) => write!(f, "the look gives nothing for the kind \"{kind}\""),
            Error::NoSimpleLook => write!(f, "the scene file has no simple look"),
            Error::Size(w, h) => write!(f, "a frame of {w}x{h} pixels is too small"),
            Error::Gpu(text) => f.write_str(text),
        }
    }
}

impl std::error::Error for Error {}
