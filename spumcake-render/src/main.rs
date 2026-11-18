//! The command line: renders one frame of a scene file to PNG files, for Spumcake-Skills and Claude Code.

use spumcake_render::{Error, Frame, Look, Renderer, Request, View, scene};
use std::process::ExitCode;

const USAGE: &str = "\
usage: spumcake-render render <scene.glb> --camera <name> --look grey|coded|simple --size <width>x<height>
                              [--time <seconds>] --out <file.png> [--depth <file.png>] [--objects <file.png>]
       spumcake-render --version

--out      the picture, an RGB PNG
--depth    the depth, a 16-bit grey PNG in millimetres along the camera's forward axis, 0 where nothing is drawn
--objects  which object each pixel shows, a 16-bit grey PNG: 0 where nothing is drawn, otherwise the object's
           number, with <file.png>.json beside it naming each number

With SPUMCAKE_RENDER_ADAPTER=software it draws with the system's software renderer, such as Mesa's lavapipe.

It exits with 0 when the files are written, 1 when something else fails, 2 when the command is not understood,
and 3, with a message starting \"no graphics device\", when it finds no GPU or software renderer to draw with.";

/// The exit code when no graphics device answers, so a caller can tell the user what to install.
const NO_DEVICE: u8 = 3;

struct Args {
    scene: String,
    camera: String,
    look: Look,
    width: u32,
    height: u32,
    time: f64,
    out: String,
    depth: Option<String>,
    objects: Option<String>,
}

fn parse(args: &[String]) -> Result<Args, String> {
    let mut it = args.iter();
    let scene = it.next().ok_or("no scene file")?.clone();
    let (mut camera, mut look, mut size, mut time, mut out, mut depth, mut objects) = (None, None, None, 0.0, None, None, None);
    while let Some(flag) = it.next() {
        let mut value = || it.next().cloned().ok_or(format!("{flag} needs a value"));
        match flag.as_str() {
            "--camera" => camera = Some(value()?),
            "--look" => {
                let v = value()?;
                look = Some(Look::parse(&v).ok_or(format!("no look called \"{v}\": use grey, coded, or simple"))?);
            }
            "--size" => {
                let v = value()?;
                let (w, h) = v.split_once('x').ok_or(format!("--size {v} is not <width>x<height>"))?;
                let w: u32 = w.parse().map_err(|_| format!("--size {v} is not <width>x<height>"))?;
                let h: u32 = h.parse().map_err(|_| format!("--size {v} is not <width>x<height>"))?;
                size = Some((w, h));
            }
            "--time" => {
                let v = value()?;
                time = v.parse().map_err(|_| format!("--time {v} is not a number of seconds"))?;
            }
            "--out" => out = Some(value()?),
            "--depth" => depth = Some(value()?),
            "--objects" => objects = Some(value()?),
            other => return Err(format!("unknown option {other}")),
        }
    }
    let (width, height) = size.ok_or("--size is required")?;
    Ok(Args {
        scene,
        camera: camera.ok_or("--camera is required")?,
        look: look.ok_or("--look is required")?,
        width,
        height,
        time,
        out: out.ok_or("--out is required")?,
        depth,
        objects,
    })
}

fn write_png(path: &str, width: u32, height: u32, colour: png::ColorType, depth: png::BitDepth, data: &[u8]) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| format!("cannot write {path}: {e}"))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(colour);
    encoder.set_depth(depth);
    let mut writer = encoder.write_header().map_err(|e| format!("cannot write {path}: {e}"))?;
    writer.write_image_data(data).map_err(|e| format!("cannot write {path}: {e}"))?;
    writer.finish().map_err(|e| format!("cannot write {path}: {e}"))
}

fn grey16(values: impl Iterator<Item = u16>) -> Vec<u8> {
    values.flat_map(u16::to_be_bytes).collect()
}

/// Why a render failed, with the exit code that says so.
struct Failure(u8, String);

impl From<String> for Failure {
    fn from(text: String) -> Failure {
        Failure(1, text)
    }
}

fn run(args: Args) -> Result<(), Failure> {
    let bytes = std::fs::read(&args.scene).map_err(|e| format!("cannot read {}: {e}", args.scene))?;
    let scene = scene::read(&bytes).map_err(|e| format!("{}: {e}", args.scene))?;
    let renderer = Renderer::new().map_err(|e| match e {
        Error::NoDevice(_) => Failure(NO_DEVICE, e.to_string()),
        e => Failure(1, e.to_string()),
    })?;
    let request =
        Request { camera: View::Named(&args.camera), look: args.look, width: args.width, height: args.height, time: args.time };
    let frame: Frame = renderer.render(&scene, &request).map_err(|e| e.to_string())?;
    let (w, h) = (frame.width, frame.height);
    write_png(&args.out, w, h, png::ColorType::Rgb, png::BitDepth::Eight, &frame.colour)?;
    if let Some(path) = &args.depth {
        let mm = frame.depth.iter().map(|&z| if z.is_finite() { (z as f64 * 1000.0).round().clamp(1.0, 65535.0) as u16 } else { 0 });
        write_png(path, w, h, png::ColorType::Grayscale, png::BitDepth::Sixteen, &grey16(mm))?;
    }
    if let Some(path) = &args.objects {
        let numbers = frame.objects.iter().map(|&o| if o == Frame::NONE { 0 } else { (o + 1) as u16 });
        write_png(path, w, h, png::ColorType::Grayscale, png::BitDepth::Sixteen, &grey16(numbers))?;
        let mut json = String::from("{\n  \"0\": null");
        for (i, object) in scene.objects.iter().enumerate() {
            json.push_str(&format!(",\n  \"{}\": {}", i + 1, quote(&object.name)));
        }
        json.push_str("\n}\n");
        let names = format!("{path}.json");
        std::fs::write(&names, json).map_err(|e| format!("cannot write {names}: {e}"))?;
    }
    eprintln!("{} {} from {} in {:?}, {w}x{h}, on {}", args.scene, args.camera, args.look_name(), args.time, renderer.adapter());
    Ok(())
}

fn quote(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

impl Args {
    fn look_name(&self) -> &'static str {
        match self.look {
            Look::Grey => "grey",
            Look::Coded => "coded",
            Look::Simple => "simple",
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version") => {
            println!("spumcake-render {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("render") => match parse(&args[1..]) {
            Err(e) => {
                eprintln!("spumcake-render: {e}\n\n{USAGE}");
                ExitCode::from(2)
            }
            Ok(args) => match run(args) {
                Ok(()) => ExitCode::SUCCESS,
                Err(Failure(code, e)) => {
                    eprintln!("spumcake-render: {e}");
                    ExitCode::from(code)
                }
            },
        },
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}
