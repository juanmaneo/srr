use std::collections::VecDeque;
use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{self, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

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

pub type Transport<'a> = dyn Fn(&str, &Value) -> Result<Value, String> + 'a;

pub fn run<I>(
    args: I,
    key: Option<&str>,
    assets: &Path,
    transport: &Transport<'_>,
) -> Result<(), String>
where
    I: IntoIterator<Item = String>,
{
    run_args(args.into_iter().collect(), key, assets, transport)
}

fn run_args(
    args: Vec<String>,
    key: Option<&str>,
    assets: &Path,
    transport: &Transport<'_>,
) -> Result<(), String> {
    let mut args = args.into_iter();
    match args.next().as_deref() {
        Some("all") => {
            let variant = required(&mut args)?;
            let output = required_path(&mut args)?;
            let seed = args.next().map(PathBuf::from);
            finish(&mut args)?;
            generate_sheet(
                &variant,
                &output,
                seed.as_deref(),
                api_key(key)?,
                assets,
                transport,
            )
        }
        Some("prepare") => {
            let variant = required(&mut args)?;
            let pose = required(&mut args)?;
            let input = required_path(&mut args)?;
            let output = required_path(&mut args)?;
            let identity = args.next().map(PathBuf::from);
            finish(&mut args)?;
            validate_variant_pose(&variant, &pose)?;
            let identity = match identity {
                Some(path) => load_image(&path)?,
                None => crop_reference(&sheet_path(assets, &variant), 0)?,
            };
            let cell = prepare_cell(&load_image(&input)?, &identity)?;
            save_png(&cell, &output)?;
            println!(
                "Prepared {} {pose} preview at {}",
                variant,
                output.display()
            );
            Ok(())
        }
        Some("single") => {
            let variant = required(&mut args)?;
            let pose = required(&mut args)?;
            let sheet = required_path(&mut args)?;
            let identity = required_path(&mut args)?;
            let output = required_path(&mut args)?;
            finish(&mut args)?;
            let pose_index = validate_variant_pose(&variant, &pose)?;
            let key = api_key(key)?;
            let identity = load_image(&identity)?;
            let generated = match pose.as_str() {
                "walk_c" | "walk_d" => {
                    let (walk_a, walk_b) = crop_walk_references(&sheet, &variant)?;
                    generate_walk_inbetween(key, &identity, &walk_a, &walk_b, &pose, transport)?
                }
                _ => {
                    let pose_reference = match image::image_dimensions(&sheet) {
                        Ok((CELL_WIDTH, CELL_HEIGHT)) => load_image(&sheet)?,
                        _ => crop_reference(&sheet, pose_index)?,
                    };
                    generate_image(key, &identity, &pose_reference, &pose, transport)?
                }
            };
            let cell = prepare_cell(&generated, &identity)?;
            save_png(&cell, &output)?;
            println!("Generated {variant}/{pose} at {}", output.display());
            Ok(())
        }
        Some("walk-cycle") => {
            let variant = required(&mut args)?;
            let sheet = required_path(&mut args)?;
            let identity = required_path(&mut args)?;
            let output = required_path(&mut args)?;
            finish(&mut args)?;
            generate_walk_cycle(
                &variant,
                &sheet,
                &identity,
                &output,
                api_key(key)?,
                transport,
            )
        }
        _ => Err(usage()),
    }
}

fn required(args: &mut std::vec::IntoIter<String>) -> Result<String, String> {
    args.next().ok_or_else(usage)
}

fn required_path(args: &mut std::vec::IntoIter<String>) -> Result<PathBuf, String> {
    required(args).map(PathBuf::from)
}

fn finish(args: &mut std::vec::IntoIter<String>) -> Result<(), String> {
    match args.next() {
        Some(_) => Err(usage()),
        None => Ok(()),
    }
}

fn api_key(key: Option<&str>) -> Result<&str, String> {
    match key {
        None => Err("OPENROUTER_API_KEY is not set in this process".to_owned()),
        Some("") => Err("OPENROUTER_API_KEY is empty".to_owned()),
        Some(key) => Ok(key),
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

fn validate_variant(variant: &str) -> Result<(), String> {
    if VARIANTS.contains(&variant) {
        Ok(())
    } else {
        Err(format!(
            "unknown variant `{variant}`; choose one of {VARIANTS:?}"
        ))
    }
}

fn validate_variant_pose(variant: &str, pose: &str) -> Result<usize, String> {
    validate_variant(variant)?;
    POSES
        .iter()
        .position(|known| *known == pose)
        .ok_or_else(|| format!("unknown pose `{pose}`; choose one of {POSES:?}"))
}

fn sheet_path(assets: &Path, variant: &str) -> PathBuf {
    assets.join(format!("{variant}.png"))
}

fn generate_sheet(
    variant: &str,
    output: &Path,
    seed: Option<&Path>,
    key: &str,
    assets: &Path,
    transport: &Transport<'_>,
) -> Result<(), String> {
    generate_sheet_with_save(variant, output, seed, key, assets, transport, &save_png)
}

fn generate_sheet_with_save(
    variant: &str,
    output: &Path,
    seed: Option<&Path>,
    key: &str,
    assets: &Path,
    transport: &Transport<'_>,
    save: &dyn Fn(&RgbaImage, &Path) -> Result<(), String>,
) -> Result<(), String> {
    validate_variant(variant)?;
    let source = sheet_path(assets, variant);
    let stem = output
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| format!("invalid output filename: {}", output.display()))?;
    let cells_dir = output.with_file_name(format!("{stem}_cells"));
    fs::create_dir_all(&cells_dir)
        .map_err(|error| format!("create {}: {error}", cells_dir.display()))?;

    let mut identity: Option<RgbaImage> = None;
    for (index, pose) in POSES.iter().enumerate() {
        let cell_path = cells_dir.join(format!("{pose}.png"));
        let cell = if cell_path.exists() {
            read_cell(&cell_path, pose)?
        } else {
            let cell = match seed {
                Some(seed_path) if index == 0 => {
                    let seed_image = load_image(seed_path)?;
                    prepare_cell(&seed_image, &seed_image)?
                }
                _ => {
                    let old_pose = crop_reference(&source, reference_pose_index(pose, index))?;
                    let reference = identity.clone().unwrap_or_else(|| old_pose.clone());
                    println!(
                        "Generating {variant}/{pose} ({}/{})...",
                        index + 1,
                        POSES.len()
                    );
                    let generated = generate_image(key, &reference, &old_pose, pose, transport)?;
                    prepare_cell(&generated, &reference)?
                }
            };
            save(&cell, &cell_path)?;
            cell
        };
        if index == 0 {
            identity = Some(cell);
        }
    }

    let sheet = assemble_sheet(&cells_dir)?;
    save(&sheet, output)?;
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
    key: &str,
    transport: &Transport<'_>,
) -> Result<(), String> {
    generate_walk_cycle_with_save(
        variant,
        sheet_path,
        identity_path,
        output_dir,
        key,
        transport,
        &save_png,
    )
}

fn generate_walk_cycle_with_save(
    variant: &str,
    sheet_path: &Path,
    identity_path: &Path,
    output_dir: &Path,
    key: &str,
    transport: &Transport<'_>,
    save: &dyn Fn(&RgbaImage, &Path) -> Result<(), String>,
) -> Result<(), String> {
    validate_variant(variant)?;
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
    let identity = load_image(identity_path)?;
    let (a_reference, b_reference) = crop_walk_references(sheet_path, variant)?;
    let prompt = "Create a polished 2D anime character WALK CYCLE sprite sheet featuring the same adult brunette woman from reference image 1. Output exactly four full-body animation frames in a strict 2-column by 2-row grid, with equal portrait cells filling the whole 3:4 canvas edge-to-edge: top-left is frame 1, top-right frame 2, bottom-left frame 3, bottom-right frame 4. The order is a seamless loop: frame 1 is the forward stride matching reference image 2; frame 2 is the passing pose as her weight moves between steps; frame 3 is the opposite stride matching reference image 3; frame 4 is the returning passing pose that leads smoothly back to frame 1. Show the same facing direction and camera in all four cells. The torso, head, hair, outfit, proportions, scale and foot baseline must stay fixed; only the natural walk-cycle limb positions and slight body bob change. Draw exactly one complete character in each cell, with no clipping and no changes of costume or face. In every frame both eyes are open and BOTH irises are clearly emerald green, matching each other. No brown, blue, hazel, gray, or mismatched irises. Use crisp clean linework and restrained cel shading. Pure solid green (#00FF00) fills every cell behind the character. No gutters, borders, panels, text, arrows, speed lines, shadows, blur, extra characters, or poses outside the four cells. This is a harmless walking animation for a game character, not a storyboard or 3D render.";
    println!("Generating one four-frame {variant} walk-cycle sheet...");
    let sheet = generate_referenced_image(
        key,
        prompt,
        &[&identity, &a_reference, &b_reference],
        transport,
    )?;
    save(&sheet, &output_dir.join("walk-cycle-source.png"))?;
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
        let frame = prepare_cell(&tile, &identity)
            .map_err(|error| format!("walk-cycle frame {index}: {error}"))?;
        save(&frame, &output_dir.join(format!("walk_{index:02}.png")))?;
        frames.push(frame);
    }
    let mut strip = RgbaImage::new(CELL_WIDTH * frames.len() as u32, CELL_HEIGHT);
    for (index, frame) in frames.iter().enumerate() {
        imageops::replace(&mut strip, frame, index as i64 * CELL_WIDTH as i64, 0);
    }
    save(&strip, &output_dir.join("walk-cycle.png"))?;
    println!(
        "Generated a {}-frame walk cycle at {}. No game assets were changed.",
        frames.len(),
        output_dir.display()
    );
    Ok(())
}

fn generate_walk_inbetween(
    key: &str,
    identity: &RgbaImage,
    walk_a: &RgbaImage,
    walk_b: &RgbaImage,
    pose: &str,
    transport: &Transport<'_>,
) -> Result<RgbaImage, String> {
    let prompt = walk_inbetween_prompt(pose)?;
    generate_referenced_image(key, prompt, &[identity, walk_a, walk_b], transport)
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
    identity_reference: &RgbaImage,
    pose_reference: &RgbaImage,
    pose: &str,
    transport: &Transport<'_>,
) -> Result<RgbaImage, String> {
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
    generate_referenced_image(
        key,
        &prompt,
        &[identity_reference, pose_reference],
        transport,
    )
}

fn generate_referenced_image(
    key: &str,
    prompt: &str,
    references: &[&RgbaImage],
    transport: &Transport<'_>,
) -> Result<RgbaImage, String> {
    let input_references: Vec<Value> = references
        .iter()
        .map(|reference| {
            json!({
                "type": "image_url",
                "image_url": {
                    "url": format!("data:image/png;base64,{}", STANDARD.encode(encode_png(reference)))
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
    let response = transport(key, &request)?;
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
    remove_green_background(decoded)
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
        enqueue_green_neighbors(&image, x, y, key, &mut seen, &mut queue);
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
            enqueue_green_neighbors(&image, x, y, key, &mut seen, &mut queue);
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

fn enqueue_green_neighbors(
    image: &RgbaImage,
    x: u32,
    y: u32,
    key: Rgba<u8>,
    seen: &mut [bool],
    queue: &mut VecDeque<usize>,
) {
    let (width, height) = image.dimensions();
    if x > 0 {
        enqueue_green(image, x - 1, y, key, seen, queue);
    }
    if x + 1 < width {
        enqueue_green(image, x + 1, y, key, seen, queue);
    }
    if y > 0 {
        enqueue_green(image, x, y - 1, key, seen, queue);
    }
    if y + 1 < height {
        enqueue_green(image, x, y + 1, key, seen, queue);
    }
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

fn prepare_cell(generated: &RgbaImage, reference: &RgbaImage) -> Result<RgbaImage, String> {
    let generated_bounds = alpha_bounds(generated)
        .ok_or_else(|| "generated image has no visible subject".to_owned())?;
    let reference_bounds = alpha_bounds(reference)
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
        generated,
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
    Ok(cell)
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

fn load_image(path: &Path) -> Result<RgbaImage, String> {
    let image = image::open(path).map_err(|error| format!("read {}: {error}", path.display()))?;
    Ok(image.to_rgba8())
}

fn crop_reference(path: &Path, pose: usize) -> Result<RgbaImage, String> {
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
    Ok(sheet
        .crop_imm(
            column * CELL_WIDTH,
            row * CELL_HEIGHT,
            CELL_WIDTH,
            CELL_HEIGHT,
        )
        .to_rgba8())
}

fn crop_walk_references(path: &Path, variant: &str) -> Result<(RgbaImage, RgbaImage), String> {
    validate_variant(variant)?;
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
    let crop = |pose: usize| {
        let column = pose as u32 % 5;
        let row = pose as u32 / 5;
        sheet
            .crop_imm(
                column * CELL_WIDTH,
                row * CELL_HEIGHT,
                CELL_WIDTH,
                CELL_HEIGHT,
            )
            .to_rgba8()
    };
    Ok((crop(1), crop(2)))
}

fn read_cell(path: &Path, pose: &str) -> Result<RgbaImage, String> {
    let image = image::open(path).map_err(|error| format!("read {}: {error}", path.display()))?;
    if image.dimensions() != (CELL_WIDTH, CELL_HEIGHT) || !image.color().has_alpha() {
        return Err(format!(
            "{pose} cell is not a transparent 240x320 PNG: {}",
            path.display()
        ));
    }
    Ok(image.to_rgba8())
}

fn assemble_sheet(cells_dir: &Path) -> Result<RgbaImage, String> {
    let mut sheet = RgbaImage::new(SHEET_WIDTH, SHEET_HEIGHT);
    for (index, pose) in POSES.iter().enumerate() {
        let cell = read_cell(&cells_dir.join(format!("{pose}.png")), pose)?;
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

fn encode_png(image: &RgbaImage) -> Vec<u8> {
    let mut png = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut png, ImageFormat::Png)
        .expect("encoding an RGBA image to memory cannot fail");
    png.into_inner()
}

static PAYLOAD_COUNTER: AtomicUsize = AtomicUsize::new(0);

pub fn request_transport<'a>(
    curl: &'a OsStr,
    payload_dir: &'a Path,
) -> impl Fn(&str, &Value) -> Result<Value, String> + 'a {
    move |key, request| send_request(curl, key, request, payload_dir)
}

pub fn send_request(
    curl: &OsStr,
    key: &str,
    request: &Value,
    payload_dir: &Path,
) -> Result<Value, String> {
    let id = PAYLOAD_COUNTER.fetch_add(1, Ordering::Relaxed);
    let payload_path = payload_dir.join(format!("srr-sprite-request-{}-{id}.json", process::id()));
    fs::write(&payload_path, request.to_string())
        .map_err(|error| format!("write temporary request: {error}"))?;
    let result = send_request_file(curl, key, &payload_path);
    let _ = fs::remove_file(payload_path);
    result
}

fn send_request_file(curl: &OsStr, key: &str, payload: &Path) -> Result<Value, String> {
    let mut child = Command::new(curl)
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
        .expect("curl stdin is piped")
        .write_all(config.as_bytes())
        .map_err(|error| format!("pass authorization to curl: {error}"))?;
    let output = child
        .wait_with_output()
        .expect("waiting for a spawned curl process cannot fail");
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
    use std::cell::Cell;
    use std::env;

    use super::*;

    const RED: Rgba<u8> = Rgba([220, 30, 30, 255]);
    const GREEN: Rgba<u8> = Rgba([0, 255, 0, 255]);

    fn test_dir(name: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("srr-sprites-{name}-{}", process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn subject_cell() -> RgbaImage {
        let mut cell = RgbaImage::new(CELL_WIDTH, CELL_HEIGHT);
        for y in 40..280 {
            for x in 40..120 {
                cell.put_pixel(x, y, RED);
            }
        }
        cell
    }

    fn write_sheet(path: &Path) {
        let mut sheet = RgbaImage::new(SHEET_WIDTH, SHEET_HEIGHT);
        for row in 0..10 {
            for column in 0..5 {
                imageops::replace(
                    &mut sheet,
                    &subject_cell(),
                    i64::from(column * CELL_WIDTH),
                    i64::from(row * CELL_HEIGHT),
                );
            }
        }
        save_png(&sheet, path).unwrap();
    }

    fn write_assets(dir: &Path, variant: &str) -> PathBuf {
        let assets = dir.join("assets");
        fs::create_dir_all(&assets).unwrap();
        write_sheet(&assets.join(format!("{variant}.png")));
        assets
    }

    fn green_screen_image() -> RgbaImage {
        let mut image = RgbaImage::from_pixel(512, 512, GREEN);
        for y in 48..464 {
            for x in 176..336 {
                image.put_pixel(x, y, RED);
            }
        }
        image
    }

    fn mock_response(image: &RgbaImage) -> Value {
        json!({
            "data": [{"b64_json": STANDARD.encode(encode_png(image))}],
            "usage": {"cost": 0.0}
        })
    }

    fn no_request(_: &str, _: &Value) -> Result<Value, String> {
        Err("unexpected request".to_owned())
    }

    fn run_args(args: &[&str], key: Option<&str>, assets: &Path) -> Result<(), String> {
        run(
            args.iter().map(|arg| (*arg).to_owned()),
            key,
            assets,
            &no_request,
        )
    }

    fn fake_curl(dir: &Path, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn image_generation_uses_the_gpt_image_model() {
        assert_eq!(MODEL, "openai/gpt-image-1");
    }

    #[test]
    fn api_key_reports_missing_and_empty_keys() {
        assert_eq!(api_key(Some("key")).unwrap(), "key");
        assert!(api_key(None).unwrap_err().contains("is not set"));
        assert!(api_key(Some("")).unwrap_err().contains("is empty"));
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
    fn variant_and_pose_validation_reject_unknown_names() {
        assert_eq!(validate_variant_pose("brunette", "stand").unwrap(), 0);
        assert!(
            validate_variant_pose("ginger", "stand")
                .unwrap_err()
                .contains("unknown variant")
        );
        assert!(
            validate_variant_pose("brunette", "dance")
                .unwrap_err()
                .contains("unknown pose")
        );
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
    fn walk_inbetween_generation_sends_identity_and_both_walk_references() {
        let identity = RgbaImage::from_pixel(4, 4, RED);
        let walk_a = RgbaImage::from_pixel(5, 5, GREEN);
        let walk_b = RgbaImage::from_pixel(6, 6, RED);
        let generated = generate_walk_inbetween(
            "test-key",
            &identity,
            &walk_a,
            &walk_b,
            "walk_c",
            &|key, request| {
                assert_eq!(key, "test-key");
                assert_eq!(request["model"], MODEL);
                assert_eq!(request["resolution"], "2K");
                assert_eq!(request["aspect_ratio"], "3:4");
                assert_eq!(request["n"], 1);
                let prompt = request["prompt"].as_str().unwrap();
                assert!(prompt.contains("Both eyes are open"));
                assert!(prompt.contains("matching emerald green"));
                let references = request["input_references"].as_array().unwrap();
                assert_eq!(references.len(), 3);
                for (reference, expected) in references.iter().zip([&identity, &walk_a, &walk_b]) {
                    let data_url = reference["image_url"]["url"].as_str().unwrap();
                    let encoded = data_url.strip_prefix("data:image/png;base64,").unwrap();
                    assert_eq!(STANDARD.decode(encoded).unwrap(), encode_png(expected));
                }
                Ok(mock_response(&green_screen_image()))
            },
        )
        .unwrap();

        assert_eq!(generated.dimensions(), (512, 512));
        assert_eq!(generated.get_pixel(0, 0)[3], 0);
        assert_eq!(generated.get_pixel(256, 256), &RED);
    }

    #[test]
    fn every_pose_prompt_is_generated_through_the_transport() {
        let image = RgbaImage::new(4, 4);
        let calls = Cell::new(0);
        let transport = |_: &str, request: &Value| {
            calls.set(calls.get() + 1);
            assert!(request["prompt"].as_str().unwrap().contains("The pose is"));
            Ok(mock_response(&green_screen_image()))
        };
        for pose in POSES {
            generate_image("test-key", &image, &image, pose, &transport).unwrap();
        }
        assert_eq!(calls.get(), POSES.len());
    }

    #[test]
    fn generated_response_without_an_image_returns_an_error() {
        let error = generate_walk_inbetween(
            "test-key",
            &RgbaImage::new(4, 4),
            &RgbaImage::new(4, 4),
            &RgbaImage::new(4, 4),
            "walk_d",
            &|_, request| {
                assert!(
                    request["prompt"]
                        .as_str()
                        .unwrap()
                        .contains("Walk B back to Walk A")
                );
                Ok(json!({"data": []}))
            },
        )
        .unwrap_err();

        assert!(error.contains("OpenRouter response has no image"));
    }

    #[test]
    fn generated_response_errors_cover_base64_png_size_and_background() {
        let image = RgbaImage::new(4, 4);
        let call = |response: Value| {
            generate_referenced_image("k", "prompt", &[&image], &|_, _| Ok(response.clone()))
        };

        assert!(
            call(json!({"data": [{"b64_json": "!!!"}]}))
                .unwrap_err()
                .contains("invalid base64")
        );
        assert!(
            call(json!({"data": [{"b64_json": STANDARD.encode(b"not png")}]}))
                .unwrap_err()
                .contains("unreadable image")
        );
        let small = STANDARD.encode(encode_png(&RgbaImage::from_pixel(100, 100, GREEN)));
        assert!(
            call(json!({"data": [{"b64_json": small}]}))
                .unwrap_err()
                .contains("unexpectedly small image")
        );
        let tall = STANDARD.encode(encode_png(&RgbaImage::from_pixel(600, 100, GREEN)));
        assert!(
            call(json!({"data": [{"b64_json": tall}]}))
                .unwrap_err()
                .contains("unexpectedly small image")
        );
        let red = STANDARD.encode(encode_png(&RgbaImage::from_pixel(512, 512, RED)));
        assert!(
            call(json!({"data": [{"b64_json": red}]}))
                .unwrap_err()
                .contains("no solid green background")
        );
    }

    #[test]
    fn removes_connected_green_background_but_preserves_isolated_green_detail() {
        let mut image = RgbaImage::from_pixel(30, 30, GREEN);
        for y in 4..26 {
            for x in 4..26 {
                image.put_pixel(x, y, RED);
            }
        }
        image.put_pixel(20, 20, GREEN);

        let result = remove_green_background(image).unwrap();

        assert_eq!(result.get_pixel(0, 0)[3], 0);
        assert_eq!(result.get_pixel(20, 20)[3], 255);
    }

    #[test]
    fn removes_large_enclosed_green_screen_patches_and_keeps_distant_greens() {
        let mut image = RgbaImage::from_pixel(30, 30, GREEN);
        for y in 3..27 {
            for x in 3..27 {
                image.put_pixel(x, y, RED);
            }
        }
        for y in 8..18 {
            for x in 8..18 {
                image.put_pixel(x, y, GREEN);
            }
        }
        image.put_pixel(1, 1, Rgba([160, 255, 0, 255]));

        let result = remove_green_background(image).unwrap();

        assert_eq!(result.get_pixel(0, 0)[3], 0);
        assert_eq!(result.get_pixel(12, 12)[3], 0);
        assert_eq!(result.get_pixel(5, 5)[3], 255);
        assert_eq!(result.get_pixel(1, 1)[3], 255);
    }

    #[test]
    fn green_background_requires_three_green_corners() {
        let mut two_corners = RgbaImage::from_pixel(30, 30, RED);
        two_corners.put_pixel(0, 0, GREEN);
        two_corners.put_pixel(29, 0, GREEN);
        assert!(remove_green_background(two_corners).is_err());

        let mut three_corners = RgbaImage::from_pixel(30, 30, GREEN);
        three_corners.put_pixel(29, 29, RED);
        assert!(remove_green_background(three_corners).is_ok());
    }

    #[test]
    fn pose_scale_tracks_stand_height_until_the_full_cell_width_is_used() {
        let mut reference = RgbaImage::new(40, 100);
        for y in 0..100 {
            for x in 10..30 {
                reference.put_pixel(x, y, RED);
            }
        }
        let mut generated = RgbaImage::new(60, 120);
        for y in 10..110 {
            for x in 15..45 {
                generated.put_pixel(x, y, RED);
            }
        }

        let prepared = prepare_cell(&generated, &reference).unwrap();
        let bounds = alpha_bounds(&prepared).unwrap();

        assert_eq!(bounds.bottom - bounds.top + 1, 125);
        assert_eq!(bounds.right - bounds.left + 1, 38);
    }

    #[test]
    fn prepare_cell_reports_missing_subjects_on_either_side() {
        let subject = RgbaImage::from_pixel(4, 4, RED);
        let empty = RgbaImage::new(4, 4);

        assert!(
            prepare_cell(&empty, &subject)
                .unwrap_err()
                .contains("generated image has no visible subject")
        );
        assert!(
            prepare_cell(&subject, &empty)
                .unwrap_err()
                .contains("source sprite cell has no visible subject")
        );
        assert!(alpha_bounds(&empty).is_none());
    }

    #[test]
    fn crop_walk_references_rejects_unknown_variants_before_reading_the_sheet() {
        let error = crop_walk_references(Path::new("missing.png"), "unknown").unwrap_err();
        assert!(error.contains("unknown variant"));
    }

    #[test]
    fn crop_reference_extracts_a_pose_cell_and_rejects_wrong_sheets() {
        let dir = test_dir("crop");
        let sheet_path = dir.join("sheet.png");
        write_sheet(&sheet_path);

        let cell = crop_reference(&sheet_path, 6).unwrap();
        assert_eq!(cell.dimensions(), (CELL_WIDTH, CELL_HEIGHT));
        assert_eq!(cell.get_pixel(60, 100), &RED);

        let small = dir.join("small.png");
        save_png(&RgbaImage::new(10, 10), &small).unwrap();
        assert!(
            crop_reference(&small, 0)
                .unwrap_err()
                .contains("must be 1200x3200")
        );
        assert!(
            crop_reference(&dir.join("missing.png"), 0)
                .unwrap_err()
                .contains("read")
        );
    }

    #[test]
    fn read_cell_requires_a_transparent_cell_size() {
        let dir = test_dir("read-cell");
        let good = dir.join("good.png");
        save_png(&subject_cell(), &good).unwrap();
        assert!(read_cell(&good, "stand").is_ok());

        let wrong_size = dir.join("wrong-size.png");
        save_png(&RgbaImage::new(10, 10), &wrong_size).unwrap();
        assert!(
            read_cell(&wrong_size, "stand")
                .unwrap_err()
                .contains("not a transparent")
        );

        let opaque = dir.join("opaque.png");
        image::RgbImage::new(CELL_WIDTH, CELL_HEIGHT)
            .save(&opaque)
            .unwrap();
        assert!(
            read_cell(&opaque, "stand")
                .unwrap_err()
                .contains("not a transparent")
        );

        assert!(
            read_cell(&dir.join("missing.png"), "stand")
                .unwrap_err()
                .contains("read")
        );
    }

    #[test]
    fn assemble_sheet_mirrors_each_cell_into_the_left_half() {
        let dir = test_dir("assemble");
        for pose in POSES {
            save_png(&subject_cell(), &dir.join(format!("{pose}.png"))).unwrap();
        }
        let sheet = assemble_sheet(&dir).unwrap();

        assert_eq!(sheet.dimensions(), (SHEET_WIDTH, SHEET_HEIGHT));
        assert_eq!(sheet.get_pixel(60, 100), &RED);
        assert_eq!(sheet.get_pixel(60, 5 * CELL_HEIGHT + 100)[3], 0);
        assert_eq!(
            sheet.get_pixel(CELL_WIDTH - 1 - 60, 5 * CELL_HEIGHT + 100),
            &RED
        );

        fs::remove_file(dir.join("walk_c.png")).unwrap();
        assert!(assemble_sheet(&dir).unwrap_err().contains("read"));
    }

    #[test]
    fn save_png_reports_directory_and_write_failures() {
        let dir = test_dir("save");
        let blocker = dir.join("blocker");
        fs::write(&blocker, b"file").unwrap();
        let image = RgbaImage::new(2, 2);

        assert!(
            save_png(&image, &blocker.join("child.png"))
                .unwrap_err()
                .contains("create")
        );
        assert!(save_png(&image, &dir).unwrap_err().contains("write"));
        assert!(
            save_png(&image, Path::new("/"))
                .unwrap_err()
                .contains("write")
        );
        save_png(&image, &dir.join("nested").join("ok.png")).unwrap();
    }

    #[test]
    fn send_request_posts_the_payload_through_curl() {
        let dir = test_dir("curl-ok");
        let curl = fake_curl(&dir, "curl", "cat >/dev/null\necho '{\"ok\":true}'");
        let transport = request_transport(curl.as_os_str(), &dir);
        let response = transport("key", &json!({"a": 1})).unwrap();
        assert_eq!(response["ok"], true);
    }

    #[test]
    fn send_request_reports_transport_failures() {
        let dir = test_dir("curl-errors");
        let failing = fake_curl(
            &dir,
            "failing",
            "cat >/dev/null\necho '{\"error\":\"denied\"}'\necho 'auth failed' >&2\nexit 22",
        );
        let error = send_request(failing.as_os_str(), "key", &json!({}), &dir).unwrap_err();
        assert!(error.contains("OpenRouter request failed"));
        assert!(error.contains("denied"));

        let invalid = fake_curl(&dir, "invalid", "cat >/dev/null\necho 'not json'");
        let error = send_request(invalid.as_os_str(), "key", &json!({}), &dir).unwrap_err();
        assert!(error.contains("invalid JSON"));

        let missing = OsStr::new("/nonexistent/srr-curl");
        let error = send_request(missing, "key", &json!({}), &dir).unwrap_err();
        assert!(error.contains("start curl"));

        let early_exit = fake_curl(&dir, "early-exit", "exit 0");
        let huge_key = "k".repeat(1 << 20);
        let error = send_request(early_exit.as_os_str(), &huge_key, &json!({}), &dir).unwrap_err();
        assert!(error.contains("pass authorization"));

        let error =
            send_request(failing.as_os_str(), "key", &json!({}), &dir.join("missing")).unwrap_err();
        assert!(error.contains("write temporary request"));
    }

    #[test]
    fn cli_usage_errors_are_reported_for_every_subcommand() {
        assert!(
            no_request("key", &json!({}))
                .unwrap_err()
                .contains("unexpected request")
        );
        let assets = Path::new("unused");
        for args in [
            vec![],
            vec!["unknown"],
            vec!["all"],
            vec!["all", "brunette"],
            vec!["all", "brunette", "out.png", "seed.png", "extra"],
            vec!["prepare"],
            vec!["prepare", "brunette", "stand", "in.png"],
            vec![
                "prepare", "brunette", "stand", "in.png", "out.png", "id.png", "extra",
            ],
            vec!["single", "brunette", "stand", "sheet.png", "id.png"],
            vec![
                "single",
                "brunette",
                "stand",
                "sheet.png",
                "id.png",
                "out.png",
                "extra",
            ],
            vec!["walk-cycle", "brunette", "sheet.png", "id.png"],
            vec![
                "walk-cycle",
                "brunette",
                "sheet.png",
                "id.png",
                "out",
                "extra",
            ],
        ] {
            let error = run_args(&args, Some("key"), assets).unwrap_err();
            assert!(error.starts_with("usage:"), "{args:?} -> {error}");
        }
    }

    #[test]
    fn cli_prepare_writes_a_cell_with_default_and_explicit_identity() {
        let dir = test_dir("cli-prepare");
        let assets = write_assets(&dir, "brunette");
        let input = dir.join("generated.png");
        save_png(&green_screen_image(), &input).unwrap();
        let output = dir.join("prepared.png");
        let input_arg = input.to_str().unwrap();
        let output_arg = output.to_str().unwrap();

        run_args(
            &["prepare", "brunette", "stand", input_arg, output_arg],
            None,
            &assets,
        )
        .unwrap();
        assert!(output.exists());

        let identity = dir.join("identity.png");
        save_png(&subject_cell(), &identity).unwrap();
        let identity_arg = identity.to_str().unwrap();
        run_args(
            &[
                "prepare",
                "brunette",
                "stand",
                input_arg,
                output_arg,
                identity_arg,
            ],
            None,
            &assets,
        )
        .unwrap();

        assert!(
            run_args(
                &["prepare", "ginger", "stand", input_arg, output_arg],
                None,
                &assets
            )
            .is_err()
        );
        assert!(
            run_args(
                &["prepare", "brunette", "dance", input_arg, output_arg],
                None,
                &assets
            )
            .is_err()
        );
        assert!(
            run_args(
                &["prepare", "brunette", "stand", "missing.png", output_arg],
                None,
                &assets
            )
            .is_err()
        );
        assert!(
            run_args(
                &[
                    "prepare",
                    "brunette",
                    "stand",
                    input_arg,
                    output_arg,
                    "missing.png"
                ],
                None,
                &assets
            )
            .is_err()
        );
        assert!(
            run_args(
                &["prepare", "brunette", "stand", input_arg, "/"],
                None,
                &assets
            )
            .is_err()
        );
        assert!(
            run_args(
                &["prepare", "brunette", "stand", input_arg, output_arg],
                None,
                &dir.join("no-assets")
            )
            .is_err()
        );
    }

    #[test]
    fn cli_single_generates_standard_and_walk_poses() {
        let dir = test_dir("cli-single");
        let sheet = dir.join("sheet.png");
        write_sheet(&sheet);
        let cell_only = dir.join("cell.png");
        save_png(&subject_cell(), &cell_only).unwrap();
        let identity = dir.join("identity.png");
        save_png(&subject_cell(), &identity).unwrap();
        let output = dir.join("out.png");
        let sheet_arg = sheet.to_str().unwrap();
        let cell_arg = cell_only.to_str().unwrap();
        let identity_arg = identity.to_str().unwrap();
        let output_arg = output.to_str().unwrap();

        let calls = Cell::new(0);
        let transport = |_: &str, _: &Value| {
            calls.set(calls.get() + 1);
            Ok(mock_response(&green_screen_image()))
        };
        let single = |args: &[&str], key: Option<&str>| {
            run(
                args.iter().map(|arg| (*arg).to_owned()),
                key,
                &dir,
                &transport,
            )
        };

        single(
            &[
                "single",
                "brunette",
                "stand",
                sheet_arg,
                identity_arg,
                output_arg,
            ],
            Some("key"),
        )
        .unwrap();
        single(
            &[
                "single",
                "brunette",
                "stand",
                cell_arg,
                identity_arg,
                output_arg,
            ],
            Some("key"),
        )
        .unwrap();
        single(
            &[
                "single",
                "brunette",
                "walk_c",
                sheet_arg,
                identity_arg,
                output_arg,
            ],
            Some("key"),
        )
        .unwrap();
        single(
            &[
                "single",
                "brunette",
                "walk_d",
                sheet_arg,
                identity_arg,
                output_arg,
            ],
            Some("key"),
        )
        .unwrap();
        assert_eq!(calls.get(), 4);
        assert!(output.exists());

        assert!(
            single(
                &[
                    "single",
                    "brunette",
                    "stand",
                    sheet_arg,
                    identity_arg,
                    output_arg
                ],
                None
            )
            .unwrap_err()
            .contains("not set")
        );
        assert!(
            single(
                &[
                    "single",
                    "brunette",
                    "stand",
                    sheet_arg,
                    identity_arg,
                    output_arg
                ],
                Some("")
            )
            .unwrap_err()
            .contains("is empty")
        );
        assert!(
            single(
                &[
                    "single",
                    "brunette",
                    "stand",
                    "missing.png",
                    identity_arg,
                    output_arg
                ],
                Some("key")
            )
            .unwrap_err()
            .contains("read")
        );
        assert!(
            single(
                &[
                    "single",
                    "brunette",
                    "walk_c",
                    "missing.png",
                    identity_arg,
                    output_arg
                ],
                Some("key")
            )
            .is_err()
        );
        assert!(
            single(
                &[
                    "single",
                    "brunette",
                    "stand",
                    sheet_arg,
                    "missing.png",
                    output_arg
                ],
                Some("key")
            )
            .unwrap_err()
            .contains("read")
        );
        assert!(
            single(
                &["single", "brunette", "stand", sheet_arg, identity_arg, "/"],
                Some("key")
            )
            .is_err()
        );
        assert!(
            single(
                &[
                    "single",
                    "ginger",
                    "stand",
                    sheet_arg,
                    identity_arg,
                    output_arg
                ],
                Some("key")
            )
            .is_err()
        );
    }

    #[test]
    fn cli_all_generates_a_full_sheet_with_and_without_a_seed() {
        let dir = test_dir("cli-all");
        let assets = write_assets(&dir, "brunette");
        let output = dir.join("sheet.png");
        let output_arg = output.to_str().unwrap();

        let calls = Cell::new(0);
        let transport = |_: &str, _: &Value| {
            calls.set(calls.get() + 1);
            Ok(mock_response(&green_screen_image()))
        };
        run(
            ["all", "brunette", output_arg].map(str::to_owned),
            Some("key"),
            &assets,
            &transport,
        )
        .unwrap();
        assert_eq!(calls.get(), POSES.len());
        let sheet = image::open(&output).unwrap();
        assert_eq!(sheet.dimensions(), (SHEET_WIDTH, SHEET_HEIGHT));

        let seed = dir.join("seed.png");
        save_png(&subject_cell(), &seed).unwrap();
        calls.set(0);
        let seeded_output = dir.join("seeded.png");
        run(
            [
                "all",
                "brunette",
                seeded_output.to_str().unwrap(),
                seed.to_str().unwrap(),
            ]
            .map(str::to_owned),
            Some("key"),
            &assets,
            &transport,
        )
        .unwrap();
        assert_eq!(calls.get(), POSES.len() - 1);
        assert!(seeded_output.exists());

        assert!(
            run_args(&["all", "brunette", output_arg], None, &assets)
                .unwrap_err()
                .contains("not set")
        );
    }

    #[test]
    fn generate_sheet_reuses_existing_cells_and_generates_the_rest() {
        let dir = test_dir("sheet-existing");
        let assets = write_assets(&dir, "brunette");
        let output = dir.join("sheet.png");
        let cells = dir.join("sheet_cells");
        fs::create_dir_all(&cells).unwrap();
        save_png(&subject_cell(), &cells.join("stand.png")).unwrap();

        let calls = Cell::new(0);
        let transport = |key: &str, request: &Value| {
            assert_eq!(key, "key");
            calls.set(calls.get() + 1);
            assert!(request["prompt"].as_str().unwrap().contains("The pose is"));
            Ok(mock_response(&green_screen_image()))
        };
        generate_sheet("brunette", &output, None, "key", &assets, &transport).unwrap();

        assert_eq!(calls.get(), POSES.len() - 1);
        assert!(output.exists());
    }

    #[test]
    fn generate_sheet_rejects_bad_inputs_and_transport_failures() {
        let dir = test_dir("sheet-errors");
        let assets = write_assets(&dir, "brunette");
        let output = dir.join("sheet.png");
        let generate = |variant: &str,
                        output: &Path,
                        seed: Option<&Path>,
                        assets: &Path,
                        transport: &Transport<'_>| {
            generate_sheet(variant, output, seed, "key", assets, transport)
        };

        assert!(
            generate("ginger", &output, None, &assets, &no_request)
                .unwrap_err()
                .contains("unknown variant")
        );
        assert!(
            generate("brunette", Path::new("/"), None, &assets, &no_request)
                .unwrap_err()
                .contains("invalid output filename")
        );

        let blocker = dir.join("blocker");
        fs::write(&blocker, b"file").unwrap();
        assert!(
            generate(
                "brunette",
                &blocker.join("sheet.png"),
                None,
                &assets,
                &no_request
            )
            .unwrap_err()
            .contains("create")
        );

        assert!(
            generate(
                "brunette",
                &output,
                None,
                &dir.join("no-assets"),
                &no_request
            )
            .unwrap_err()
            .contains("read")
        );

        let wrong_size_assets = dir.join("wrong-assets");
        fs::create_dir_all(&wrong_size_assets).unwrap();
        save_png(
            &RgbaImage::new(10, 10),
            &wrong_size_assets.join("brunette.png"),
        )
        .unwrap();
        assert!(
            generate(
                "brunette",
                &dir.join("wrong.png"),
                None,
                &wrong_size_assets,
                &no_request
            )
            .unwrap_err()
            .contains("must be")
        );

        let bad_cells = dir.join("bad_cells");
        fs::create_dir_all(&bad_cells).unwrap();
        save_png(&RgbaImage::new(10, 10), &bad_cells.join("stand.png")).unwrap();
        assert!(
            generate("brunette", &dir.join("bad.png"), None, &assets, &no_request)
                .unwrap_err()
                .contains("not a transparent")
        );

        let missing_seed = dir.join("missing-seed.png");
        assert!(
            generate(
                "brunette",
                &dir.join("seeded-missing.png"),
                Some(&missing_seed),
                &assets,
                &no_request
            )
            .unwrap_err()
            .contains("read")
        );

        assert!(
            generate(
                "brunette",
                &dir.join("failed.png"),
                None,
                &assets,
                &|_, _| Err("boom".to_owned())
            )
            .unwrap_err()
            .contains("boom")
        );

        let blank = |_: &str, _: &Value| Ok(mock_response(&RgbaImage::from_pixel(512, 512, GREEN)));
        assert!(
            generate("brunette", &dir.join("blank.png"), None, &assets, &blank)
                .unwrap_err()
                .contains("no visible subject")
        );
    }

    #[test]
    fn walk_cycle_writes_four_frames_and_a_strip() {
        let dir = test_dir("walk-ok");
        let assets = write_assets(&dir, "brunette");
        let identity = dir.join("identity.png");
        save_png(&subject_cell(), &identity).unwrap();
        let out = dir.join("walk");
        let transport = |_: &str, _: &Value| {
            let mut sheet = RgbaImage::from_pixel(512, 684, GREEN);
            for y in 100..600 {
                for x in 150..360 {
                    sheet.put_pixel(x, y, RED);
                }
            }
            Ok(mock_response(&sheet))
        };
        let sheet = sheet_path(&assets, "brunette");

        generate_walk_cycle("brunette", &sheet, &identity, &out, "key", &transport).unwrap();

        for index in 0..4 {
            assert!(out.join(format!("walk_{index:02}.png")).exists());
        }
        assert!(out.join("walk-cycle-source.png").exists());
        let strip = image::open(out.join("walk-cycle.png")).unwrap();
        assert_eq!(strip.dimensions(), (CELL_WIDTH * 4, CELL_HEIGHT));
    }

    #[test]
    fn walk_cycle_rejects_bad_grids_bad_frames_and_bad_outputs() {
        let dir = test_dir("walk-errors");
        let assets = write_assets(&dir, "brunette");
        let sheet = sheet_path(&assets, "brunette");
        let identity = dir.join("identity.png");
        save_png(&subject_cell(), &identity).unwrap();

        let sized = |width: u32, height: u32| {
            move |_: &str, _: &Value| {
                Ok(mock_response(&RgbaImage::from_pixel(width, height, GREEN)))
            }
        };
        for (width, height) in [(512, 512), (513, 684), (512, 685)] {
            let error = generate_walk_cycle(
                "brunette",
                &sheet,
                &identity,
                &dir.join(format!("bad-{width}-{height}")),
                "key",
                &sized(width, height),
            )
            .unwrap_err();
            assert!(error.contains("near-3:4"), "{width}x{height}: {error}");
        }

        let failure = generate_walk_cycle(
            "brunette",
            &sheet,
            &identity,
            &dir.join("transport-error"),
            "key",
            &|_, _| Err("mock transport error".to_owned()),
        )
        .unwrap_err();
        assert!(failure.contains("mock transport error"));

        let top_left_only = |_: &str, _: &Value| {
            let mut sheet = RgbaImage::from_pixel(512, 684, GREEN);
            for y in 0..200 {
                for x in 0..200 {
                    sheet.put_pixel(x, y, RED);
                }
            }
            Ok(mock_response(&sheet))
        };
        let error = generate_walk_cycle(
            "brunette",
            &sheet,
            &identity,
            &dir.join("sparse"),
            "key",
            &top_left_only,
        )
        .unwrap_err();
        assert!(error.contains("walk-cycle frame 1"));

        let full = dir.join("full");
        fs::create_dir_all(&full).unwrap();
        fs::write(full.join("keep.txt"), b"x").unwrap();
        assert!(
            generate_walk_cycle("brunette", &sheet, &identity, &full, "key", &no_request)
                .unwrap_err()
                .contains("not empty")
        );

        let file_output = dir.join("file-output");
        fs::write(&file_output, b"x").unwrap();
        assert!(
            generate_walk_cycle(
                "brunette",
                &sheet,
                &identity,
                &file_output,
                "key",
                &no_request
            )
            .unwrap_err()
            .contains("read")
        );

        let blocker = dir.join("blocker");
        fs::write(&blocker, b"x").unwrap();
        assert!(
            generate_walk_cycle(
                "brunette",
                &sheet,
                &identity,
                &blocker.join("out"),
                "key",
                &no_request
            )
            .unwrap_err()
            .contains("create")
        );

        assert!(
            generate_walk_cycle(
                "ginger",
                &sheet,
                &identity,
                &dir.join("x"),
                "key",
                &no_request
            )
            .unwrap_err()
            .contains("unknown variant")
        );
        assert!(
            generate_walk_cycle(
                "brunette",
                &sheet,
                &dir.join("missing.png"),
                &dir.join("y"),
                "key",
                &no_request
            )
            .unwrap_err()
            .contains("read")
        );
    }

    #[test]
    fn cli_walk_cycle_runs_end_to_end_with_a_mocked_transport() {
        let dir = test_dir("cli-walk");
        let assets = write_assets(&dir, "brunette");
        let identity = dir.join("identity.png");
        save_png(&subject_cell(), &identity).unwrap();
        let out = dir.join("walk-out");
        let sheet = sheet_path(&assets, "brunette");
        let walk_transport = |_: &str, _: &Value| {
            let mut generated = RgbaImage::from_pixel(512, 684, GREEN);
            for y in 100..600 {
                for x in 150..360 {
                    generated.put_pixel(x, y, RED);
                }
            }
            Ok(mock_response(&generated))
        };
        run(
            [
                "walk-cycle",
                "brunette",
                sheet.to_str().unwrap(),
                identity.to_str().unwrap(),
                out.to_str().unwrap(),
            ]
            .map(str::to_owned),
            Some("key"),
            &assets,
            &walk_transport,
        )
        .unwrap();
        assert!(out.join("walk-cycle.png").exists());
        assert!(
            run_args(
                &["walk-cycle", "brunette", "x.png", "y.png", "z"],
                None,
                &assets
            )
            .unwrap_err()
            .contains("not set")
        );
    }

    #[test]
    fn cli_required_arguments_and_error_propagation_are_covered() {
        let dir = test_dir("cli-errors");
        let assets = write_assets(&dir, "brunette");
        let identity = dir.join("identity.png");
        save_png(&subject_cell(), &identity).unwrap();
        let identity_arg = identity.to_str().unwrap();
        for args in [
            vec!["prepare", "brunette"],
            vec!["prepare", "brunette", "stand"],
            vec!["single", "brunette"],
            vec!["single", "brunette", "stand"],
            vec!["single", "brunette", "stand", "sheet.png"],
            vec!["single", "brunette", "stand", "sheet.png", "id.png"],
            vec!["walk-cycle"],
            vec!["walk-cycle", "brunette"],
            vec!["walk-cycle", "brunette", "sheet.png"],
        ] {
            assert!(
                run_args(&args, Some("key"), &assets)
                    .unwrap_err()
                    .starts_with("usage:")
            );
        }

        assert!(
            run_args(&["single"], Some("key"), &assets)
                .unwrap_err()
                .starts_with("usage:")
        );

        let transparent = dir.join("transparent.png");
        save_png(&RgbaImage::new(4, 4), &transparent).unwrap();
        assert!(
            run_args(
                &[
                    "prepare",
                    "brunette",
                    "stand",
                    transparent.to_str().unwrap(),
                    "out.png"
                ],
                None,
                &assets,
            )
            .unwrap_err()
            .contains("generated image has no visible subject")
        );

        let sheet = sheet_path(&assets, "brunette");
        let output = dir.join("out.png");
        let output_arg = output.to_str().unwrap();
        let id_cell = dir.join("rgb-identity.png");
        image::RgbImage::new(CELL_WIDTH, CELL_HEIGHT)
            .save(&id_cell)
            .unwrap();
        let id_cell_arg = id_cell.to_str().unwrap();
        let truncated = dir.join("truncated.png");
        let mut png = Vec::new();
        subject_cell()
            .write_to(&mut std::io::Cursor::new(&mut png), ImageFormat::Png)
            .unwrap();
        png[41] = 0xff;
        fs::write(&truncated, &png).unwrap();
        assert!(image::image_dimensions(&truncated).is_ok());
        assert!(
            run_args(
                &[
                    "single",
                    "brunette",
                    "stand",
                    truncated.to_str().unwrap(),
                    identity_arg,
                    output_arg
                ],
                Some("key"),
                &assets,
            )
            .unwrap_err()
            .contains("read")
        );
        assert!(
            run_args(
                &[
                    "single",
                    "brunette",
                    "stand",
                    id_cell_arg,
                    identity_arg,
                    output_arg
                ],
                Some("key"),
                &assets,
            )
            .is_err()
        );

        let sheet_arg = sheet.to_str().unwrap();
        let fail_transport = |_: &str, _: &Value| Err("mock transport error".to_owned());
        assert!(
            run(
                [
                    "single",
                    "brunette",
                    "stand",
                    sheet_arg,
                    identity_arg,
                    output_arg
                ]
                .map(str::to_owned),
                Some("key"),
                &assets,
                &fail_transport,
            )
            .unwrap_err()
            .contains("mock transport error")
        );

        assert!(
            run(
                [
                    "single",
                    "brunette",
                    "walk_c",
                    sheet_arg,
                    identity_arg,
                    output_arg
                ]
                .map(str::to_owned),
                Some("key"),
                &assets,
                &fail_transport,
            )
            .unwrap_err()
            .contains("mock transport error")
        );

        let empty_response =
            |_: &str, _: &Value| Ok(mock_response(&RgbaImage::from_pixel(512, 512, GREEN)));
        assert!(
            run(
                [
                    "single",
                    "brunette",
                    "stand",
                    sheet_arg,
                    identity_arg,
                    output_arg
                ]
                .map(str::to_owned),
                Some("key"),
                &assets,
                &empty_response,
            )
            .unwrap_err()
            .contains("generated image has no visible subject")
        );

        let blank_seed = dir.join("blank-seed.png");
        save_png(&RgbaImage::new(4, 4), &blank_seed).unwrap();
        assert!(
            generate_sheet(
                "brunette",
                &dir.join("blank-seeded.png"),
                Some(&blank_seed),
                "key",
                &assets,
                &no_request,
            )
            .unwrap_err()
            .contains("generated image has no visible subject")
        );

        let wrong_sheet = dir.join("wrong-sheet.png");
        save_png(&RgbaImage::new(10, 10), &wrong_sheet).unwrap();
        for pose in ["walk_c", "walk_d"] {
            assert!(
                generate_walk_cycle(
                    "brunette",
                    &wrong_sheet,
                    &identity,
                    &dir.join(format!("bad-sheet-{pose}")),
                    "key",
                    &no_request,
                )
                .unwrap_err()
                .contains("must be 1200x3200")
            );
        }
        assert!(
            generate_walk_inbetween(
                "key",
                &subject_cell(),
                &subject_cell(),
                &subject_cell(),
                "stand",
                &no_request,
            )
            .unwrap_err()
            .contains("must be `walk_c` or `walk_d`")
        );
    }

    #[test]
    fn generators_propagate_injected_write_failures() {
        let dir = test_dir("save-injection");
        let assets = write_assets(&dir, "brunette");
        let seed = dir.join("seed.png");
        save_png(&subject_cell(), &seed).unwrap();
        let transport = |_: &str, _: &Value| Ok(mock_response(&green_screen_image()));
        let fail_save = |_: &RgbaImage, _: &Path| Err("mock save failure".to_owned());

        assert!(
            generate_sheet_with_save(
                "brunette",
                &dir.join("cell-failure.png"),
                Some(&seed),
                "key",
                &assets,
                &no_request,
                &fail_save,
            )
            .unwrap_err()
            .contains("mock save failure")
        );

        let missing_cell = |_: &RgbaImage, path: &Path| {
            if path.file_name().is_some_and(|name| name == "stand.png") {
                Ok(())
            } else {
                save_png(&subject_cell(), path)
            }
        };
        let generation_transport = |_: &str, _: &Value| Ok(mock_response(&green_screen_image()));
        assert!(
            generate_sheet_with_save(
                "brunette",
                &dir.join("assembly-failure.png"),
                Some(&seed),
                "key",
                &assets,
                &generation_transport,
                &missing_cell,
            )
            .unwrap_err()
            .contains("read")
        );

        let final_sheet = dir.join("output-failure.png");
        let fail_final_sheet = |image: &RgbaImage, path: &Path| {
            if path == final_sheet {
                Err("final sheet write failed".to_owned())
            } else {
                save_png(image, path)
            }
        };
        assert!(
            generate_sheet_with_save(
                "brunette",
                &final_sheet,
                Some(&seed),
                "key",
                &assets,
                &transport,
                &fail_final_sheet,
            )
            .unwrap_err()
            .contains("final sheet write failed")
        );

        let sheet = sheet_path(&assets, "brunette");
        let identity = dir.join("identity.png");
        save_png(&subject_cell(), &identity).unwrap();
        let walk_transport = |_: &str, _: &Value| {
            let mut generated = RgbaImage::from_pixel(512, 684, GREEN);
            for y in 100..600 {
                for x in 150..360 {
                    generated.put_pixel(x, y, RED);
                }
            }
            Ok(mock_response(&generated))
        };
        for failure in ["walk-cycle-source.png", "walk_00.png", "walk-cycle.png"] {
            let output = dir.join(format!("walk-fail-{failure}"));
            let save_for_failure = |image: &RgbaImage, path: &Path| {
                if path.file_name().is_some_and(|name| name == failure) {
                    Err(format!("failed to write {failure}"))
                } else {
                    save_png(image, path)
                }
            };
            assert!(
                generate_walk_cycle_with_save(
                    "brunette",
                    &sheet,
                    &identity,
                    &output,
                    "key",
                    &walk_transport,
                    &save_for_failure,
                )
                .unwrap_err()
                .contains(failure)
            );
        }
    }
}
