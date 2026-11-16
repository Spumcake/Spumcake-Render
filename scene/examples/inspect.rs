//! Reads scene files and prints what each holds: its builder, its objects,
//! its cameras with their published renders, its looks, and its swing.
//!
//! Usage: `cargo run -p scene --example inspect -- <scene.glb>...`

fn main() {
    let mut failed = false;
    for path in std::env::args().skip(1) {
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) => {
                eprintln!("{path}: {e}");
                failed = true;
                continue;
            }
        };
        match scene::read(&bytes) {
            Ok(scene) => {
                let triangles: usize = scene.objects.iter().map(|o| o.positions.len() / 3).sum();
                let hinged = scene.objects.iter().filter(|o| o.under_hinge.is_some()).count();
                println!("{path}");
                println!("  builder {} ({}…)", scene.builder, &scene.builder_sha256[..12]);
                println!("  {} objects, {triangles} triangles, {hinged} on the hinge, image {:?}", scene.objects.len(), scene.image_size);
                for camera in &scene.cameras {
                    println!("  camera {} hfov {} render {}", camera.name, camera.hfov, camera.reference_file.as_deref().unwrap_or("none"));
                }
                let looks = &scene.looks;
                println!("  looks: {} greys, {} colours, edges {}/{}/{:?}, simple {}", looks.grey.len(), looks.coded.len(),
                    looks.edges.grey, looks.edges.coded, looks.edges.simple,
                    looks.simple.as_ref().map(|s| format!("keeps {}", s.kept.len())).unwrap_or("none".into()));
                match &scene.swing {
                    Some(swing) => println!("  swing {}: {} keys to {:.3} s, hinge {:?}, {:.1}° at 3.333 s, {:.1}° at the end",
                        swing.name, swing.times.len(), swing.length(), swing.hinge, swing.angle_at(3.333), swing.angle_at(swing.length())),
                    None => println!("  no swing: {}", scene.door_note.as_deref().unwrap_or("")),
                }
            }
            Err(e) => {
                eprintln!("{path}: {e}");
                failed = true;
            }
        }
    }
    if failed {
        std::process::exit(1);
    }
}
