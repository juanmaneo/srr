use std::collections::VecDeque;
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use image::imageops::{self, FilterType};
use image::{GenericImageView, ImageFormat, Rgba, RgbaImage};
use serde_json::{Value, json};

const MODEL: &str = "openai/gpt-image-1";
const IMAGE_API: &str = "https://openrouter.ai/api/v1/images";
const VARIANTS: [&str; 10] = [
    "brunette", "blonde", "redhead", "pink", "white", "black", "green", "violet", "cyan", "raven",
];
const POSES: [&str; 24] = [
    "stand",
    "walk_a",
    "walk_b",
    "crouch",
    "block",
    "punch_low",
    "punch_mid",
    "punch_high",
    "kick_low",
    "kick_mid",
    "kick_high",
    "jump",
    "jump_kick",
    "hit",
    "grab",
    "throw",
    "sweep",
    "special_rush",
    "special_fireball",
    "special_palm",
    "special_slide",
    "ko",
    "walk_c",
    "walk_d",
];
const POSE_DESCRIPTIONS: [&str; 24] = [
    "standing in a compact fighting guard",
    "walking forward, leading foot forward",
    "walking forward, opposite step of the walk cycle",
    "crouching low with guard raised",
    "stepping backward while blocking",
    "throwing a low punch",
    "throwing a straight mid-height punch",
    "throwing a high punch",
    "throwing a crouched low kick, sweeping the foot just above ankle height",
    "throwing an upright mid kick, extending the foot at hip or waist height",
    "throwing a high kick, raising the foot to head or shoulder height",
    "jumping with knees bent",
    "performing a kick while airborne",
    "recoiling after being hit",
    "reaching forward to grab an opponent",
    "performing the shoulder throw",
    "performing a flying scissor sweep",
    "rushing forward in a shoulder charge",
    "throwing a glowing fireball",
    "performing an armored palm strike",
    "performing a low sliding tackle",
    "lying knocked out on her side, body stretched horizontally",
    "the passing walk pose between the leading-foot stride and the opposite stride",
    "the passing walk pose between the opposite stride and the leading-foot stride",
];
const CELL_WIDTH: u32 = 240;
const CELL_HEIGHT: u32 = 320;
const CELL_MARGIN: u32 = 4;
const SIZE_BOOST: f64 = 1.25;
const SHEET_WIDTH: u32 = CELL_WIDTH * 5;
const SHEET_HEIGHT: u32 = CELL_HEIGHT * 10;

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("all") => {
            let variant = args.next().ok_or_else(usage)?;
            let output = args.next().map(PathBuf::from).ok_or_else(usage)?;
            let seed = args.next().map(PathBuf::from);
            if args.next().is_some() {
                return Err(usage());
            }
            generate_sheet(&variant, &output, seed.as_deref())
        }
        Some("prepare") => {
            let variant = args.next().ok_or_else(usage)?;
            let pose = args.next().ok_or_else(usage)?;
            let input = args.next().map(PathBuf::from).ok_or_else(usage)?;
            let output = args.next().map(PathBuf::from).ok_or_else(usage)?;
            let identity = args.next().map(PathBuf::from);
            if args.next().is_some() {
                return Err(usage());
            }
            validate_variant_pose(&variant, &pose)?;
            let identity = match identity {
                Some(path) => fs::read(&path)
                    .map_err(|error| format!("read identity {}: {error}", path.display()))?,
                None => crop_reference(&sheet_path(&variant), 0)?,
            };
            let cell = prepare_cell(
                &fs::read(&input).map_err(|e| format!("read {}: {e}", input.display()))?,
                &identity,
            )?;
            let cell = image::load_from_memory(&cell)
                .map_err(|error| format!("decode prepared cell: {error}"))?
                .to_rgba8();
            save_png(&cell, &output)?;
            println!(
                "Prepared {} {pose} preview at {}",
                variant,
                output.display()
            );
            Ok(())
        }
        Some("single") => {
            let variant = args.next().ok_or_else(usage)?;
            let pose = args.next().ok_or_else(usage)?;
            let sheet = args.next().map(PathBuf::from).ok_or_else(usage)?;
            let identity = args.next().map(PathBuf::from).ok_or_else(usage)?;
            let output = args.next().map(PathBuf::from).ok_or_else(usage)?;
            if args.next().is_some() {
                return Err(usage());
            }
            let pose_index = validate_variant_pose(&variant, &pose)?;
            let key = env::var("OPENROUTER_API_KEY")
                .map_err(|_| "OPENROUTER_API_KEY is not set in this process".to_owned())?;
            let identity =
                fs::read(&identity).map_err(|error| format!("read identity reference: {error}"))?;
            let generated = match pose.as_str() {
                "walk_c" | "walk_d" => {
                    let walk_a =
                        crop_reference(&sheet, validate_variant_pose(&variant, "walk_a")?)?;
                    let walk_b =
                        crop_reference(&sheet, validate_variant_pose(&variant, "walk_b")?)?;
                    generate_walk_inbetween(&key, &identity, &walk_a, &walk_b, &pose)?
                }
                _ => {
                    let pose_reference = match image::image_dimensions(&sheet) {
                        Ok((CELL_WIDTH, CELL_HEIGHT)) => fs::read(&sheet).map_err(|error| {
                            format!("read pose reference {}: {error}", sheet.display())
                        })?,
                        _ => crop_reference(&sheet, pose_index)?,
                    };
                    generate_image(&key, &identity, &pose_reference, &pose)?
                }
            };
            let cell = prepare_cell(&generated, &identity)?;
            let cell = image::load_from_memory(&cell)
                .map_err(|error| format!("decode generated cell: {error}"))?
                .to_rgba8();
            save_png(&cell, &output)?;
            println!("Generated {variant}/{pose} at {}", output.display());
            Ok(())
        }
        Some("walk-cycle") => {
            let variant = args.next().ok_or_else(usage)?;
            let sheet = args.next().map(PathBuf::from).ok_or_else(usage)?;
            let identity = args.next().map(PathBuf::from).ok_or_else(usage)?;
            let output = args.next().map(PathBuf::from).ok_or_else(usage)?;
            if args.next().is_some() {
                return Err(usage());
            }
            generate_walk_cycle(&variant, &sheet, &identity, &output)
        }
        _ => Err(usage()),
    }
}

fn usage() -> String {
    format!(
        "usage:\n  cargo run --manifest-path tools/sprites/Cargo.toml -- all <variant> <sheet.png> [stand-reference.png]\n  cargo run --manifest-path tools/sprites/Cargo.toml -- single <variant> <pose> <pose-sheet.png> <identity.png> <cell.png>\n  cargo run --manifest-path tools/sprites/Cargo.toml -- walk-cycle <variant> <pose-sheet.png> <identity.png> <output-directory>\n  cargo run --manifest-path tools/sprites/Cargo.toml -- prepare <variant> <pose> <generated.png> <cell.png> [identity.png]\nvariants: {VARIANTS:?}\nposes: {POSES:?}"
    )
}

fn reference_pose_index(pose: &str, index: usize) -> usize {
    match pose {
        "walk_c" => 1,
        "walk_d" => 2,
        _ => index,
    }
}

fn validate_variant_pose(variant: &str, pose: &str) -> Result<usize, String> {
    if !VARIANTS.contains(&variant) {
        return Err(format!(
            "unknown variant `{variant}`; choose one of {VARIANTS:?}"
        ));
    }
    POSES
        .iter()
        .position(|known| *known == pose)
        .ok_or_else(|| format!("unknown pose `{pose}`; choose one of {POSES:?}"))
}

fn sheet_path(variant: &str) -> PathBuf {
    PathBuf::from(format!("assets/skins/manga/{variant}.png"))
}

fn generate_sheet(variant: &str, output: &Path, seed: Option<&Path>) -> Result<(), String> {
    if !VARIANTS.contains(&variant) {
        return Err(format!(
            "unknown variant `{variant}`; choose one of {VARIANTS:?}"
        ));
    }
    let source = sheet_path(variant);
    let stem = output
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| format!("invalid output filename: {}", output.display()))?;
    let cells_dir = output.with_file_name(format!("{stem}_cells"));
    fs::create_dir_all(&cells_dir)
        .map_err(|error| format!("create {}: {error}", cells_dir.display()))?;

    let key = env::var("OPENROUTER_API_KEY")
        .map_err(|_| "OPENROUTER_API_KEY is not set in this process".to_owned())?;
    if key.is_empty() {
        return Err("OPENROUTER_API_KEY is empty".to_owned());
    }

    let mut identity_reference: Option<Vec<u8>> = None;
    for (index, pose) in POSES.iter().enumerate() {
        let cell_path = cells_dir.join(format!("{pose}.png"));
        let old_pose = crop_reference(&source, reference_pose_index(pose, index))?;
        let new_cell = if cell_path.exists() {
            let bytes = fs::read(&cell_path)
                .map_err(|error| format!("read {}: {error}", cell_path.display()))?;
            validate_cell(&bytes, pose)?;
            bytes
        } else if index == 0
            && let Some(seed_path) = seed
        {
            let bytes = fs::read(seed_path)
                .map_err(|error| format!("read seed {}: {error}", seed_path.display()))?;
            prepare_cell(&bytes, &bytes)?
        } else {
            let identity = identity_reference.as_deref().unwrap_or(old_pose.as_slice());
            println!(
                "Generating {variant}/{pose} ({}/{})...",
                index + 1,
                POSES.len()
            );
            let generated = generate_image(&key, identity, &old_pose, pose)?;
            prepare_cell(&generated, identity)?
        };
        save_png(
            &image::load_from_memory(&new_cell)
                .map_err(|e| e.to_string())?
                .to_rgba8(),
            &cell_path,
        )?;
        if index == 0 {
            identity_reference = Some(new_cell.clone());
        }
    }

    let sheet = assemble_sheet(&cells_dir)?;
    save_png(&sheet, output)?;
    println!(
        "Assembled transparent {}x{} sheet at {}. The original asset was not changed.",
        SHEET_WIDTH,
        SHEET_HEIGHT,
        output.display()
    );
    Ok(())
}

fn generate_walk_cycle(
    variant: &str,
    sheet_path: &Path,
    identity_path: &Path,
    output_dir: &Path,
) -> Result<(), String> {
    if !VARIANTS.contains(&variant) {
        return Err(format!(
            "unknown variant `{variant}`; choose one of {VARIANTS:?}"
        ));
    }
    if output_dir.exists()
        && fs::read_dir(output_dir)
            .map_err(|error| format!("read {}: {error}", output_dir.display()))?
            .next()
            .is_some()
    {
        return Err(format!(
            "output directory is not empty: {}",
            output_dir.display()
        ));
    }
    fs::create_dir_all(output_dir)
        .map_err(|error| format!("create {}: {error}", output_dir.display()))?;
    let key = env::var("OPENROUTER_API_KEY")
        .map_err(|_| "OPENROUTER_API_KEY is not set in this process".to_owned())?;
    if key.is_empty() {
        return Err("OPENROUTER_API_KEY is empty".to_owned());
    }
    let identity = fs::read(identity_path)
        .map_err(|error| format!("read identity {}: {error}", identity_path.display()))?;
    let a_reference = crop_reference(sheet_path, validate_variant_pose(variant, "walk_a")?)?;
    let b_reference = crop_reference(sheet_path, validate_variant_pose(variant, "walk_b")?)?;
    let prompt = "Create a polished 2D anime character WALK CYCLE sprite sheet featuring the same adult brunette woman from reference image 1. Output exactly four full-body animation frames in a strict 2-column by 2-row grid, with equal portrait cells filling the whole 3:4 canvas edge-to-edge: top-left is frame 1, top-right frame 2, bottom-left frame 3, bottom-right frame 4. The order is a seamless loop: frame 1 is the forward stride matching reference image 2; frame 2 is the passing pose as her weight moves between steps; frame 3 is the opposite stride matching reference image 3; frame 4 is the returning passing pose that leads smoothly back to frame 1. Show the same facing direction and camera in all four cells. The torso, head, hair, outfit, proportions, scale and foot baseline must stay fixed; only the natural walk-cycle limb positions and slight body bob change. Draw exactly one complete character in each cell, with no clipping and no changes of costume or face. In every frame both eyes are open and BOTH irises are clearly emerald green, matching each other. No brown, blue, hazel, gray, or mismatched irises. Use crisp clean linework and restrained cel shading. Pure solid green (#00FF00) fills every cell behind the character. No gutters, borders, panels, text, arrows, speed lines, shadows, blur, extra characters, or poses outside the four cells. This is a harmless walking animation for a game character, not a storyboard or 3D render.";
    println!("Generating one four-frame {variant} walk-cycle sheet...");
    let generated =
        generate_referenced_image(&key, prompt, &[&identity, &a_reference, &b_reference])?;
    let sheet = image::load_from_memory(&generated)
        .map_err(|error| format!("decode generated walk-cycle sheet: {error}"))?
        .to_rgba8();
    save_png(&sheet, &output_dir.join("walk-cycle-source.png"))?;
    let (width, height) = sheet.dimensions();
    let aspect_error = ((width as i64 * 4) - (height as i64 * 3)).unsigned_abs();
    if width % 2 != 0 || height % 2 != 0 || aspect_error * 100 > height as u64 * 3 {
        return Err(format!(
            "expected a near-3:4 image with a divisible 2x2 grid, received {width}x{height}"
        ));
    }
    let cell_width = width / 2;
    let cell_height = height / 2;
    let mut frames = Vec::with_capacity(4);
    for (index, (x, y)) in [
        (0, 0),
        (cell_width, 0),
        (0, cell_height),
        (cell_width, cell_height),
    ]
    .into_iter()
    .enumerate()
    {
        let tile = imageops::crop_imm(&sheet, x, y, cell_width, cell_height).to_image();
        let tile = prepare_cell(&encode_png(&tile)?, &identity)?;
        let frame = image::load_from_memory(&tile)
            .map_err(|error| format!("decode walk-cycle frame {index}: {error}"))?
            .to_rgba8();
        save_png(&frame, &output_dir.join(format!("walk_{index:02}.png")))?;
        frames.push(frame);
    }
    let mut strip = RgbaImage::new(CELL_WIDTH * frames.len() as u32, CELL_HEIGHT);
    for (index, frame) in frames.iter().enumerate() {
        imageops::replace(&mut strip, frame, index as i64 * CELL_WIDTH as i64, 0);
    }
    save_png(&strip, &output_dir.join("walk-cycle.png"))?;
    println!(
        "Generated a {}-frame walk cycle at {}. No game assets were changed.",
        frames.len(),
        output_dir.display()
    );
    Ok(())
}

fn generate_walk_inbetween(
    key: &str,
    identity: &[u8],
    walk_a: &[u8],
    walk_b: &[u8],
    pose: &str,
) -> Result<Vec<u8>, String> {
    let prompt = walk_inbetween_prompt(pose)?;
    generate_referenced_image(key, prompt, &[identity, walk_a, walk_b])
}

fn walk_inbetween_prompt(pose: &str) -> Result<&'static str, String> {
    match pose {
        "walk_c" => Ok(
            "Create exactly one full-body 2D anime game sprite of the same adult brunette woman. Reference image 1 defines her identity, face, hair, costume, colors and proportions. Reference image 2 is her Walk A stride; reference image 3 is her opposite Walk B stride. Draw the distinct passing frame halfway from Walk A to Walk B: keep her weight over the same leg that is forward in Walk A, with that support foot planted beneath her hips, while the other leg passes naturally under her body toward its next forward step. Her arms swing halfway between the two references in the matching direction. Preserve the same facing direction, camera, character scale, foot baseline, torso, head, hairstyle and outfit; change only the walk-cycle limb positions and subtle body bob. Both eyes are open and both irises are clearly matching emerald green. This must be a natural in-between frame, not a copy of either stride, a standing pose, or a mirrored character. Draw one clean, anatomically plausible figure with crisp linework and restrained cel shading on a pure solid green (#00FF00) background. No text, border, shadows, speed lines, blur, scenery, panels, extra figures or duplicated limbs. This is one still from a seamless fighting-game walk cycle.",
        ),
        "walk_d" => Ok(
            "Create exactly one full-body 2D anime game sprite of the same adult brunette woman. Reference image 1 defines her identity, face, hair, costume, colors and proportions. Reference image 2 is her Walk A stride; reference image 3 is her opposite Walk B stride. Draw the distinct passing frame halfway from Walk B back to Walk A: keep her weight over the same leg that is forward in Walk B, with that support foot planted beneath her hips, while the other leg passes naturally under her body toward its next forward step. Her arms swing halfway between the two references in the opposite transition direction. Preserve the same facing direction, camera, character scale, foot baseline, torso, head, hairstyle and outfit; change only the walk-cycle limb positions and subtle body bob. Both eyes are open and both irises are clearly matching emerald green. This must be a natural in-between frame, not a copy of either stride, a standing pose, or a mirrored character. Draw one clean, anatomically plausible figure with crisp linework and restrained cel shading on a pure solid green (#00FF00) background. No text, border, shadows, speed lines, blur, scenery, panels, extra figures or duplicated limbs. This is one still from a seamless fighting-game walk cycle.",
        ),
        _ => Err(format!(
            "walk in-between pose must be `walk_c` or `walk_d`, received `{pose}`"
        )),
    }
}

fn generate_image(
    key: &str,
    identity_reference: &[u8],
    pose_reference: &[u8],
    pose: &str,
) -> Result<Vec<u8>, String> {
    let pose_detail = match pose {
        "ko" => {
            "The exact action is a knockout: she has fallen onto her side and lies horizontally on the ground, with her head and shoulders toward the right side of the frame and her legs and boots extending toward the left. Her eyes are closed and her limbs rest naturally on the ground. Keep the pose strongly asymmetrical so horizontal mirroring makes her lie in the opposite direction. Do not draw her flat on her back, star-shaped, sitting, standing, crouching, kneeling, or hovering."
        }
        "walk_b" => {
            "Draw the opposite phase of the walk cycle from reference image 2. Keep the same upright torso, body scale, and facing direction. Reverse the leg stride clearly: the leg that reaches forward in reference image 2 now trails behind, while the other leg swings forward with the knee bent. Reverse the natural arm swing as well, with the opposite arm forward. Keep a visible walking stride; do not repeat the same foot positions, stand with feet together, or mirror the whole character. Both eyes are matching green."
        }
        "walk_c" => {
            "Draw the natural passing pose halfway between the leading-foot stride and the opposite stride. Keep her torso, head, hair, outfit, scale, floor baseline and facing direction exactly consistent with the walk references. Her weight passes over the supporting foot while the other leg travels underneath her hips; the arms move halfway between their two walk-reference positions. This must be a distinct in-between frame, not a copy of either stride or a mirrored character. Both open irises are matching emerald green."
        }
        "walk_d" => {
            "Draw the natural passing pose halfway between the opposite stride and the leading-foot stride. Keep her torso, head, hair, outfit, scale, floor baseline and facing direction exactly consistent with the walk references. Her weight passes over the supporting foot while the other leg travels underneath her hips; the arms move halfway between their two walk-reference positions. This must be a distinct in-between frame, not a copy of either stride or a mirrored character. Both open irises are matching emerald green."
        }
        "punch_low" => {
            "Make a low punch using reference image 2 as the exact body pose: preserve its deep crouch, lowered hips, torso angle, and both leg positions. Change only the arms. Draw exactly two arms total: one shoulder connects to a single arm punching straight forward toward the right at low chest height, ending in one fist; the other shoulder connects to one bent arm with its fist guarding the chest. This is a punch, not a two-arm block. Both arms must be clearly connected at the shoulders. No extra hands, fists, or arms at the waist or hip; the extended punch fist and the chest-guard fist are the only hands. Ignore the arm positions in both references; keep reference 2's crouched torso and legs."
        }
        "punch_mid" => {
            "Draw an upright mid-height punch. Keep her torso vertical and legs in a standing fighting stance, matching the full-height scale of the stand reference. One arm punches straight forward from its shoulder at shoulder height, with the forearm nearly horizontal and the fist clearly in front of her body. The other arm bends with its fist guarding the chest. Exactly two arms and two hands total, each attached at the correct shoulder; no extra, hidden, or duplicated limbs. This is a punch, not a block."
        }
        "punch_high" => {
            "Draw an upright high punch. Keep her torso vertical and legs in a standing fighting stance, matching the full-height scale of the stand reference. One arm punches forward and upward from its shoulder, with the fist at head or forehead height, clearly higher than a shoulder-height mid punch. The other arm bends with its fist guarding the chest. Exactly two arms and two hands total, each attached at the correct shoulder; no extra, hidden, or duplicated limbs. This is a punch, not a block."
        }
        "kick_low" => {
            "This is the crouched low version of the mid kick: keep the torso and hips low, bend the supporting knee deeply, and sweep the kicking boot forward just above ankle height. The leg must not rise to waist or chest height. The bent crouch is essential so this silhouette is clearly lower than the upright mid kick."
        }
        "kick_mid" => {
            "This is an upright mid kick: stand tall with the torso vertical and the supporting leg straight. The kicking leg is extended nearly horizontally, with the boot at hip or waist height, well below the shoulders. Do not crouch, and do not raise the leg past waist height."
        }
        "kick_high" => {
            "This is a high kick: stand tall with the torso upright and the supporting leg straight. The kicking leg is raised steeply, with the boot at head or shoulder height, clearly higher than a mid kick. Do not crouch, and do not keep the kick at waist height."
        }
        _ => {
            "Faithfully redraw the pose reference action without changing it. Ignore the identity reference's pose and limb positions; use it only for the character's identity and design."
        }
    };
    let effects = match pose {
        "special_fireball" => {
            "Include only the single fireball visible in the pose reference; no speed lines or limb-shaped effects."
        }
        "special_palm" => {
            "Include only the palm energy effect visible in the pose reference; no speed lines or limb-shaped effects."
        }
        _ => "No effects, speed lines, action streaks, glow, blur, or duplicate silhouettes.",
    };
    let prompt = format!(
        "Create one polished production-quality 2D anime fighting-game sprite. Reference image 1 provides only the girl's identity: face, hairstyle, hair length and color, costume, palette and body proportions. EYE COLOR IS A STRICT IDENTITY REQUIREMENT: whenever her eyes are open, clearly draw BOTH irises vivid emerald green, matching one another. Do not make either iris brown, blue, gray, hazel or a different color; do not let highlights or shadows obscure the green iris color. Keep both eyes consistent in every generated pose. For the knockout pose only, her eyes are closed as specified. Ignore reference image 1's pose, especially arm and hand positions. Reference image 2 controls the exact action, facing direction, camera view and position of every limb. Combine identity from image 1 with pose from image 2. {} The pose is {}. Keep the character large and full-sized in the frame: use nearly all available frame height and width with only a small clear margin; do not shrink crouching or airborne poses. Keep the same head-to-body ratio, limb thickness, hair volume, face and outfit as image 1; do not redesign or chibi-ify her. Exactly one human figure with one head, two arms, two hands, two legs and two feet, attached in anatomically correct positions. Never add, duplicate, detach, fuse, hide-and-replace, or echo a limb. Hands must be clean, readable and attached at the wrists. {} Draw one crisp full-body still frame with clean linework, believable anatomy, expressive face and restrained cel shading. Do not reproduce the reference images as panels, thumbnails, insets, extra figures or extra poses. Solid pure green (#00FF00) background for automatic removal; no shadows, scenery, text, border, action marks or extra characters. This is a 2D game sprite, not a photo or 3D render.",
        pose_detail,
        POSE_DESCRIPTIONS[POSES.iter().position(|known| *known == pose).unwrap()],
        effects
    );
    generate_referenced_image(key, &prompt, &[identity_reference, pose_reference])
}

fn generate_referenced_image(
    key: &str,
    prompt: &str,
    references: &[&[u8]],
) -> Result<Vec<u8>, String> {
    let input_references: Vec<Value> = references
        .iter()
        .map(|reference| {
            json!({
                "type": "image_url",
                "image_url": {
                    "url": format!("data:image/png;base64,{}", STANDARD.encode(reference))
                }
            })
        })
        .collect();
    let request = json!({
        "model": MODEL,
        "prompt": prompt,
        "resolution": "2K",
        "aspect_ratio": "3:4",
        "output_format": "png",
        "background": "opaque",
        "n": 1,
        "input_references": input_references
    });
    let response = send_request(key, &request)?;
    if let Some(cost) = response.pointer("/usage/cost") {
        println!("  OpenRouter reported cost: ${cost}");
    }
    let encoded = response
        .get("data")
        .and_then(Value::as_array)
        .and_then(|images| images.first())
        .and_then(|image| image.get("b64_json"))
        .and_then(Value::as_str)
        .ok_or_else(|| format!("OpenRouter response has no image: {response}"))?;
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|error| format!("invalid base64 image response: {error}"))?;
    let decoded = image::load_from_memory(&bytes)
        .map_err(|error| format!("OpenRouter returned an unreadable image: {error}"))?
        .to_rgba8();
    let (width, height) = decoded.dimensions();
    if width < 512 || height < 512 {
        return Err(format!("unexpectedly small image: {width}x{height}"));
    }
    let transparent = remove_green_background(decoded)?;
    encode_png(&transparent)
}

fn remove_green_background(mut image: RgbaImage) -> Result<RgbaImage, String> {
    let (width, height) = image.dimensions();
    let corners = [
        *image.get_pixel(0, 0),
        *image.get_pixel(width - 1, 0),
        *image.get_pixel(0, height - 1),
        *image.get_pixel(width - 1, height - 1),
    ];
    let green_corners: Vec<_> = corners
        .iter()
        .filter(|pixel| {
            pixel[1] > pixel[0].saturating_add(40) && pixel[1] > pixel[2].saturating_add(40)
        })
        .collect();
    if green_corners.len() < 3 {
        return Err("image has no solid green background to remove".to_owned());
    }
    let key = Rgba([
        (green_corners
            .iter()
            .map(|pixel| u32::from(pixel[0]))
            .sum::<u32>()
            / green_corners.len() as u32) as u8,
        (green_corners
            .iter()
            .map(|pixel| u32::from(pixel[1]))
            .sum::<u32>()
            / green_corners.len() as u32) as u8,
        (green_corners
            .iter()
            .map(|pixel| u32::from(pixel[2]))
            .sum::<u32>()
            / green_corners.len() as u32) as u8,
        255,
    ]);

    let mut seen = vec![false; width as usize * height as usize];
    let mut queue = VecDeque::new();
    for x in 0..width {
        enqueue_green(&image, x, 0, key, &mut seen, &mut queue);
        enqueue_green(&image, x, height - 1, key, &mut seen, &mut queue);
    }
    for y in 0..height {
        enqueue_green(&image, 0, y, key, &mut seen, &mut queue);
        enqueue_green(&image, width - 1, y, key, &mut seen, &mut queue);
    }

    while let Some(index) = queue.pop_front() {
        let x = index as u32 % width;
        let y = index as u32 / width;
        image.put_pixel(x, y, Rgba([0, 0, 0, 0]));
        if x > 0 {
            enqueue_green(&image, x - 1, y, key, &mut seen, &mut queue);
        }
        if x + 1 < width {
            enqueue_green(&image, x + 1, y, key, &mut seen, &mut queue);
        }
        if y > 0 {
            enqueue_green(&image, x, y - 1, key, &mut seen, &mut queue);
        }
        if y + 1 < height {
            enqueue_green(&image, x, y + 1, key, &mut seen, &mut queue);
        }
    }

    let min_component_size = ((width as usize * height as usize) / 10_000).clamp(64, 4_096);
    for start in 0..seen.len() {
        if seen[start] {
            continue;
        }
        let x = start as u32 % width;
        let y = start as u32 / width;
        enqueue_green(&image, x, y, key, &mut seen, &mut queue);
        if queue.is_empty() {
            continue;
        }

        let mut component = Vec::new();
        while let Some(index) = queue.pop_front() {
            component.push(index);
            let x = index as u32 % width;
            let y = index as u32 / width;
            if x > 0 {
                enqueue_green(&image, x - 1, y, key, &mut seen, &mut queue);
            }
            if x + 1 < width {
                enqueue_green(&image, x + 1, y, key, &mut seen, &mut queue);
            }
            if y > 0 {
                enqueue_green(&image, x, y - 1, key, &mut seen, &mut queue);
            }
            if y + 1 < height {
                enqueue_green(&image, x, y + 1, key, &mut seen, &mut queue);
            }
        }
        if component.len() >= min_component_size {
            for index in component {
                image.put_pixel(
                    index as u32 % width,
                    index as u32 / width,
                    Rgba([0, 0, 0, 0]),
                );
            }
        }
    }
    Ok(image)
}

fn enqueue_green(
    image: &RgbaImage,
    x: u32,
    y: u32,
    key: Rgba<u8>,
    seen: &mut [bool],
    queue: &mut VecDeque<usize>,
) {
    let width = image.width() as usize;
    let index = y as usize * width + x as usize;
    let pixel = image.get_pixel(x, y);
    let green_screen_pixel = pixel[1] > pixel[0].saturating_add(20)
        && pixel[1] > pixel[2].saturating_add(20)
        && (0..3).all(|channel| pixel[channel].abs_diff(key[channel]) <= 150);
    if !seen[index] && green_screen_pixel {
        seen[index] = true;
        queue.push_back(index);
    }
}

fn prepare_cell(generated: &[u8], reference: &[u8]) -> Result<Vec<u8>, String> {
    let generated = image::load_from_memory(generated)
        .map_err(|error| format!("decode generated image: {error}"))?
        .to_rgba8();
    let reference = image::load_from_memory(reference)
        .map_err(|error| format!("decode source cell: {error}"))?
        .to_rgba8();
    let generated_bounds = alpha_bounds(&generated)
        .ok_or_else(|| "generated image has no visible subject".to_owned())?;
    let reference_bounds = alpha_bounds(&reference)
        .ok_or_else(|| "source sprite cell has no visible subject".to_owned())?;

    let source_width = generated_bounds.right - generated_bounds.left + 1;
    let source_height = generated_bounds.bottom - generated_bounds.top + 1;
    let target_height = reference_bounds.bottom - reference_bounds.top + 1;
    let reference_scale = target_height as f64 / source_height as f64;
    let limit_scale = (f64::from(CELL_WIDTH - 2 * CELL_MARGIN) / source_width as f64)
        .min(f64::from(CELL_HEIGHT - 2 * CELL_MARGIN) / source_height as f64);
    let scale = (reference_scale * SIZE_BOOST).min(limit_scale);
    let width = ((source_width as f64 * scale).round() as u32).clamp(1, CELL_WIDTH);
    let height = ((source_height as f64 * scale).round() as u32).clamp(1, CELL_HEIGHT);
    let crop = imageops::crop_imm(
        &generated,
        generated_bounds.left,
        generated_bounds.top,
        source_width,
        source_height,
    )
    .to_image();
    let resized = imageops::resize(&crop, width, height, FilterType::Lanczos3);

    let left = (CELL_WIDTH - width) / 2;
    let top = CELL_HEIGHT - height;
    let mut cell = RgbaImage::new(CELL_WIDTH, CELL_HEIGHT);
    imageops::replace(&mut cell, &resized, i64::from(left), i64::from(top));
    encode_png(&cell)
}

#[derive(Clone, Copy)]
struct Bounds {
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
}

fn alpha_bounds(image: &RgbaImage) -> Option<Bounds> {
    let mut bounds = Bounds {
        left: image.width(),
        top: image.height(),
        right: 0,
        bottom: 0,
    };
    let mut found = false;
    for (x, y, pixel) in image.enumerate_pixels() {
        if pixel[3] > 16 {
            found = true;
            bounds.left = bounds.left.min(x);
            bounds.top = bounds.top.min(y);
            bounds.right = bounds.right.max(x);
            bounds.bottom = bounds.bottom.max(y);
        }
    }
    found.then_some(bounds)
}

fn crop_reference(path: &Path, pose: usize) -> Result<Vec<u8>, String> {
    let sheet = image::open(path).map_err(|error| format!("read {}: {error}", path.display()))?;
    if sheet.dimensions() != (SHEET_WIDTH, SHEET_HEIGHT) {
        return Err(format!(
            "{} must be {}x{}, got {}x{}",
            path.display(),
            SHEET_WIDTH,
            SHEET_HEIGHT,
            sheet.width(),
            sheet.height()
        ));
    }
    let column = pose as u32 % 5;
    let row = pose as u32 / 5;
    let cell = sheet.crop_imm(
        column * CELL_WIDTH,
        row * CELL_HEIGHT,
        CELL_WIDTH,
        CELL_HEIGHT,
    );
    let mut png = std::io::Cursor::new(Vec::new());
    cell.write_to(&mut png, ImageFormat::Png)
        .map_err(|error| format!("encode reference cell: {error}"))?;
    Ok(png.into_inner())
}

fn validate_cell(bytes: &[u8], pose: &str) -> Result<(), String> {
    let image = image::load_from_memory(bytes)
        .map_err(|error| format!("decode existing {pose} cell: {error}"))?;
    if image.dimensions() != (CELL_WIDTH, CELL_HEIGHT) || !image.color().has_alpha() {
        return Err(format!(
            "existing {pose} cell is not transparent 240x320 PNG"
        ));
    }
    Ok(())
}

fn assemble_sheet(cells_dir: &Path) -> Result<RgbaImage, String> {
    let mut sheet = RgbaImage::new(SHEET_WIDTH, SHEET_HEIGHT);
    for (index, pose) in POSES.iter().enumerate() {
        let path = cells_dir.join(format!("{pose}.png"));
        let cell = image::open(&path)
            .map_err(|error| format!("read {}: {error}", path.display()))?
            .to_rgba8();
        if cell.dimensions() != (CELL_WIDTH, CELL_HEIGHT) {
            return Err(format!("{} is not a 240x320 cell", path.display()));
        }
        let column = index as u32 % 5;
        let row = index as u32 / 5;
        imageops::replace(
            &mut sheet,
            &cell,
            i64::from(column * CELL_WIDTH),
            i64::from(row * CELL_HEIGHT),
        );
        let mirrored = imageops::flip_horizontal(&cell);
        imageops::replace(
            &mut sheet,
            &mirrored,
            i64::from(column * CELL_WIDTH),
            i64::from((row + 5) * CELL_HEIGHT),
        );
    }
    Ok(sheet)
}

fn save_png(image: &RgbaImage, path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("create {}: {error}", parent.display()))?;
    }
    image
        .save_with_format(path, ImageFormat::Png)
        .map_err(|error| format!("write {}: {error}", path.display()))
}

fn encode_png(image: &RgbaImage) -> Result<Vec<u8>, String> {
    let mut png = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut png, ImageFormat::Png)
        .map_err(|error| format!("encode PNG: {error}"))?;
    Ok(png.into_inner())
}

fn send_request(key: &str, request: &Value) -> Result<Value, String> {
    let payload_path =
        env::temp_dir().join(format!("srr-sprite-request-{}.json", std::process::id()));
    fs::write(&payload_path, request.to_string())
        .map_err(|error| format!("write temporary request: {error}"))?;
    let result = send_request_file(key, &payload_path);
    let _ = fs::remove_file(payload_path);
    result
}

fn send_request_file(key: &str, payload: &Path) -> Result<Value, String> {
    let mut child = Command::new("curl")
        .args([
            "--silent",
            "--show-error",
            "--fail-with-body",
            "--request",
            "POST",
            IMAGE_API,
            "--header",
            "Content-Type: application/json",
            "--data-binary",
        ])
        .arg(format!("@{}", payload.display()))
        .args(["--config", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("start curl: {error}"))?;

    let config = format!("header = \"Authorization: Bearer {key}\"\n");
    child
        .stdin
        .take()
        .ok_or_else(|| "curl stdin unavailable".to_owned())?
        .write_all(config.as_bytes())
        .map_err(|error| format!("pass authorization to curl: {error}"))?;
    let output = child
        .wait_with_output()
        .map_err(|error| format!("wait for OpenRouter: {error}"))?;
    if !output.status.success() {
        let body = String::from_utf8_lossy(&output.stdout);
        let detail = String::from_utf8_lossy(&output.stderr);
        return Err(format!("OpenRouter request failed: {body} {detail}"));
    }
    serde_json::from_slice(&output.stdout).map_err(|error| {
        format!(
            "OpenRouter returned invalid JSON: {error}: {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_generation_uses_the_gpt_image_model() {
        assert_eq!(MODEL, "openai/gpt-image-1");
    }

    #[test]
    fn walk_inbetween_cells_reuse_the_existing_walk_reference_poses() {
        assert_eq!(reference_pose_index("walk_c", 22), 1);
        assert_eq!(reference_pose_index("walk_d", 23), 2);
        assert_eq!(reference_pose_index("walk_a", 1), 1);
        assert_eq!(reference_pose_index("ko", 21), 21);
        assert_eq!(POSES.len(), POSE_DESCRIPTIONS.len());
        assert_eq!(POSES[22], "walk_c");
        assert_eq!(POSES[23], "walk_d");
    }

    #[test]
    fn walk_inbetween_prompts_distinguish_both_transitions_and_reject_other_poses() {
        let walk_c = walk_inbetween_prompt("walk_c").unwrap();
        let walk_d = walk_inbetween_prompt("walk_d").unwrap();

        assert_ne!(walk_c, walk_d);
        for prompt in [walk_c, walk_d] {
            assert!(prompt.contains("Reference image 2 is her Walk A stride"));
            assert!(prompt.contains("reference image 3 is her opposite Walk B stride"));
            assert!(prompt.contains("both irises are clearly matching emerald green"));
            assert!(prompt.contains("natural in-between frame"));
        }
        assert!(walk_inbetween_prompt("walk_a").is_err());
    }

    #[test]
    fn removes_connected_green_background_but_preserves_isolated_green_detail() {
        let green = Rgba([0, 255, 0, 255]);
        let opaque_red = Rgba([220, 30, 30, 255]);
        let mut image = RgbaImage::from_pixel(30, 30, green);
        for y in 4..26 {
            for x in 4..26 {
                image.put_pixel(x, y, opaque_red);
            }
        }
        image.put_pixel(20, 20, green);

        let result = remove_green_background(image).unwrap();

        assert_eq!(result.get_pixel(0, 0)[3], 0);
        assert_eq!(result.get_pixel(20, 20)[3], 255);
    }

    #[test]
    fn pose_scale_tracks_stand_height_until_the_full_cell_width_is_used() {
        let mut reference = RgbaImage::new(40, 100);
        for y in 0..100 {
            for x in 10..30 {
                reference.put_pixel(x, y, Rgba([200, 20, 20, 255]));
            }
        }
        let mut generated = RgbaImage::new(60, 120);
        for y in 10..110 {
            for x in 15..45 {
                generated.put_pixel(x, y, Rgba([200, 20, 20, 255]));
            }
        }

        let prepared = prepare_cell(
            &encode_png(&generated).unwrap(),
            &encode_png(&reference).unwrap(),
        )
        .unwrap();
        let prepared = image::load_from_memory(&prepared).unwrap().to_rgba8();
        let bounds = alpha_bounds(&prepared).unwrap();

        assert_eq!(bounds.bottom - bounds.top + 1, 125);
        assert_eq!(bounds.right - bounds.left + 1, 38);
    }

    #[test]
    fn removes_large_enclosed_green_screen_patches() {
        let green = Rgba([0, 255, 0, 255]);
        let opaque_red = Rgba([220, 30, 30, 255]);
        let mut image = RgbaImage::from_pixel(30, 30, green);
        for y in 3..27 {
            for x in 3..27 {
                image.put_pixel(x, y, opaque_red);
            }
        }
        for y in 8..18 {
            for x in 8..18 {
                image.put_pixel(x, y, green);
            }
        }

        let result = remove_green_background(image).unwrap();

        assert_eq!(result.get_pixel(0, 0)[3], 0);
        assert_eq!(result.get_pixel(12, 12)[3], 0);
        assert_eq!(result.get_pixel(5, 5)[3], 255);
    }
}
