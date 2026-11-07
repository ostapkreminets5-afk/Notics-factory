use noctics_shared::{AssetKind, Medium};
use serde_json::{Map, Value, json};

use crate::comfy::{Folder, Inventory};

const IMAGE_NODES: [&str; 4] = ["CLIPTextEncode", "VAEDecode", "SaveImage", "KSampler"];
const MATTING_NODES: [&str; 4] = [
    "LoadBackgroundRemovalModel",
    "RemoveBackground",
    "InvertMask",
    "JoinImageWithAlpha",
];
const Z_IMAGE_NODES: [&str; 6] = [
    "UNETLoader",
    "CLIPLoader",
    "VAELoader",
    "ModelSamplingAuraFlow",
    "ConditioningZeroOut",
    "EmptySD3LatentImage",
];
const FLUX2_NODES: [&str; 10] = [
    "UNETLoader",
    "CLIPLoader",
    "VAELoader",
    "EmptyFlux2LatentImage",
    "KSamplerSelect",
    "Flux2Scheduler",
    "ConditioningZeroOut",
    "CFGGuider",
    "RandomNoise",
    "SamplerCustomAdvanced",
];
const CHECKPOINT_NODES: [&str; 2] = ["CheckpointLoaderSimple", "EmptyLatentImage"];
const ACE_NODES: [&str; 7] = [
    "ModelSamplingAuraFlow",
    "TextEncodeAceStepAudio1.5",
    "ConditioningZeroOut",
    "EmptyAceStep1.5LatentAudio",
    "KSampler",
    "VAEDecodeAudio",
    "SaveAudioAdvanced",
];
const STABLE_AUDIO_NODES: [&str; 7] = [
    "CheckpointLoaderSimple",
    "CLIPLoader",
    "CLIPTextEncode",
    "EmptyLatentAudio",
    "KSampler",
    "VAEDecodeAudio",
    "SaveAudioAdvanced",
];
const MESH_NODES: [&str; 24] = [
    "LoadImage",
    "LoadBackgroundRemovalModel",
    "RemoveBackground",
    "ImageCropToMask",
    "CLIPVisionLoader",
    "UNETLoader",
    "VAELoader",
    "CFGOverride",
    "RescaleCFG",
    "ModelSamplingSD3",
    "EmptyTrellis2LatentStructure",
    "KSampler",
    "VaeDecodeStructureTrellis2",
    "Trellis2ShapeStage",
    "Trellis2UpsampleStage",
    "VaeDecodeShapeTrellis",
    "Trellis2TextureStage",
    "VaeDecodeTextureTrellis",
    "DecimateMesh",
    "MeshSmoothNormals",
    "UnwrapMesh",
    "BakeTextureFromVoxel",
    "ApplyTextureToMesh",
    "SaveGLB",
];
const TRELLIS_NODES: [&str; 1] = ["Trellis2Conditioning"];
const PIXAL_NODES: [&str; 7] = [
    "Pixal3DConditioning",
    "LoadMoGeModel",
    "MoGeInference",
    "MoGeGeometryToFOV",
    "RemeshMesh",
    "BakeNormalMapFromMesh",
    "BakeAmbientOcclusion",
];
const ULTRATEX_NODES: [&str; 12] = [
    "LoadImage",
    "LoadBackgroundRemovalModel",
    "RemoveBackground",
    "UNETLoader",
    "CLIPLoader",
    "VAELoader",
    "CLIPTextEncode",
    "UltraTexLoraLoader",
    "UltraTexForegroundDecoder",
    "UltraTexPrep",
    "UltraTexSampler",
    "UltraTexBake",
];
const PRECISIONS: [&str; 10] = [
    "nvfp4", "fp4", "int8", "fp8", "bf16", "fp16", "fp32", "gguf", "q8", "q4",
];
const NON_IMAGE_CHECKPOINTS: [&str; 12] = [
    "video", "ltx", "svd", "wan2", "wan_2", "zero123", "hunyuan", "audio", "refiner", "inpaint",
    "mochi", "cosmos",
];
const XL_MARKERS: [&str; 5] = ["xl", "pony", "illustrious", "noob", "juggernaut"];
const NEGATIVE_FALLBACK: &str = "lowres, blurry, jpeg artifacts, watermark, signature, text, letters, deformed, extra limbs, cropped";
const DEFAULT_KEYSCALE: &str = "C minor";
const MESH_FACES: u32 = 30_000;
const MESH_TEXTURE: u32 = 2048;
const ULTRATEX_VIEW_SIZE: u32 = 1024;
const ULTRATEX_RENDER_SIZE: u32 = 2048;
const ULTRATEX_STEPS: u32 = 25;
const ULTRATEX_GUIDANCE: f64 = 2.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recipe {
    ZImage {
        unet: String,
        clip: String,
        vae: String,
        turbo: bool,
    },
    Flux2 {
        unet: String,
        clip: String,
        vae: String,
        distilled: bool,
    },
    Checkpoint {
        file: String,
        xl: bool,
    },
    AceAllInOne {
        file: String,
        turbo: bool,
    },
    AceSplit {
        unet: String,
        small: String,
        large: String,
        vae: String,
        turbo: bool,
    },
    StableAudio {
        file: String,
        clip: String,
    },
    Trellis {
        unet: String,
        shape: String,
        texture: String,
        vision: String,
        matting: String,
    },
    Pixal {
        unet: String,
        shape: String,
        texture: String,
        vision: String,
        matting: String,
        geometry: String,
    },
    UltraTex {
        unet: String,
        clip: String,
        vae: String,
        lora: String,
        decoder: String,
        matting: String,
    },
}

impl Recipe {
    pub fn family(&self) -> &'static str {
        match self {
            Self::ZImage { .. } => "z-image",
            Self::Flux2 { .. } => "flux2",
            Self::Checkpoint { .. } => "checkpoint",
            Self::AceAllInOne { .. } | Self::AceSplit { .. } => "ace-step",
            Self::StableAudio { .. } => "stable-audio",
            Self::Trellis { .. } => "trellis",
            Self::Pixal { .. } => "pixal3d",
            Self::UltraTex { .. } => "ultratex",
        }
    }

    pub fn needs_mesh(&self) -> bool {
        matches!(self, Self::UltraTex { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub id: String,
    pub name: String,
    pub note: String,
    pub precision: Option<String>,
    pub medium: Medium,
    pub rank: u8,
    pub recipe: Option<Recipe>,
    pub missing: Vec<String>,
}

impl Candidate {
    pub fn ready(&self) -> bool {
        self.recipe.is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Aspect {
    Square,
    Portrait,
    Landscape,
}

impl Aspect {
    pub fn from_code(code: &str) -> Self {
        match code {
            "portrait" => Self::Portrait,
            "landscape" => Self::Landscape,
            _ => Self::Square,
        }
    }

    fn size(self, large: bool) -> (u32, u32) {
        match (self, large) {
            (Self::Square, true) => (1024, 1024),
            (Self::Portrait, true) => (832, 1216),
            (Self::Landscape, true) => (1216, 832),
            (Self::Square, false) => (512, 512),
            (Self::Portrait, false) => (512, 768),
            (Self::Landscape, false) => (768, 512),
        }
    }
}

pub struct Brief<'a> {
    pub kind: AssetKind,
    pub prompt: &'a str,
    pub tags: &'a str,
    pub negative: &'a str,
    pub aspect: Aspect,
    pub seconds: f64,
    pub bpm: u32,
    pub keyscale: &'a str,
    pub lyrics: &'a str,
    pub seed: u64,
    pub prefix: &'a str,
    pub matting: Option<&'a str>,
    pub reference: Option<&'a str>,
    pub mesh: Option<&'a str>,
}

pub fn roster(inventory: &Inventory) -> Vec<Candidate> {
    let mut found = Vec::new();
    for unet in inventory.in_folder(Folder::DiffusionModels) {
        let name = base(unet);
        let candidate = if is_z_image(&name) {
            Some(z_image(inventory, unet, &name))
        } else if is_flux2_klein(&name) {
            Some(flux2(inventory, unet, &name))
        } else if is_ace(&name) {
            Some(ace_split(inventory, unet, &name))
        } else if name.contains("pixal3d") {
            Some(pixal(inventory, unet))
        } else if name.contains("trellis") {
            Some(trellis(inventory, unet))
        } else {
            None
        };
        found.extend(candidate.map(|candidate| with_precision(candidate, &name)));
    }
    for file in inventory.in_folder(Folder::Checkpoints) {
        let name = base(file);
        let candidate = if is_ace(&name) {
            Some(ace_all_in_one(inventory, file, &name))
        } else if is_stable_audio_open(&name) {
            Some(stable_audio(inventory, file))
        } else if NON_IMAGE_CHECKPOINTS.iter().any(|marker| name.contains(marker)) {
            None
        } else {
            Some(checkpoint(inventory, file, &name))
        };
        found.extend(candidate.map(|candidate| with_precision(candidate, &name)));
    }
    found.extend(
        inventory
            .in_folder(Folder::Loras)
            .iter()
            .filter(|lora| is_ultratex_albedo(&base(lora)))
            .map(|lora| ultratex(inventory, lora)),
    );
    found.sort_by(|left, right| {
        medium_order(left.medium)
            .cmp(&medium_order(right.medium))
            .then(left.rank.cmp(&right.rank))
            .then(left.name.cmp(&right.name))
    });
    found
}

pub fn matting_model(inventory: &Inventory) -> Option<String> {
    let usable = MATTING_NODES.iter().all(|node| inventory.has_node(node));
    usable
        .then(|| prefer(inventory.in_folder(Folder::BackgroundRemoval), "birefnet"))
        .flatten()
}

fn medium_order(medium: Medium) -> u8 {
    match medium {
        Medium::Image => 0,
        Medium::Mesh => 1,
        Medium::Audio => 2,
    }
}

fn base(file: &str) -> String {
    file.rsplit(['/', '\\'])
        .next()
        .unwrap_or(file)
        .to_ascii_lowercase()
}

fn is_z_image(name: &str) -> bool {
    name.contains("z_image") || name.contains("z-image") || name.contains("zimage")
}

fn is_flux2_klein(name: &str) -> bool {
    (name.contains("flux-2") || name.contains("flux2") || name.contains("flux_2")) && name.contains("klein")
}

fn is_ace(name: &str) -> bool {
    name.contains("acestep") || name.contains("ace_step") || name.contains("ace-step")
}

fn is_stable_audio_open(name: &str) -> bool {
    (name.contains("stable_audio") || name.contains("stable-audio")) && name.contains("open")
}

fn is_ultratex_albedo(name: &str) -> bool {
    name.contains("ultratex") && !name.contains("_mr") && !name.contains("flux1")
}

fn is_klein_base_9b(name: &str) -> bool {
    is_flux2_klein(name) && name.contains("base") && name.contains("9b")
}

fn precision(name: &str) -> Option<&'static str> {
    PRECISIONS.into_iter().find(|marker| name.contains(marker))
}

fn with_precision(mut candidate: Candidate, name: &str) -> Candidate {
    candidate.precision = precision(name).map(str::to_owned);
    candidate
}

fn prefer(files: &[String], marker: &str) -> Option<String> {
    files
        .iter()
        .find(|file| base(file).contains(marker))
        .or_else(|| files.first())
        .cloned()
}

struct Needs<'a> {
    inventory: &'a Inventory,
    missing: Vec<String>,
}

impl<'a> Needs<'a> {
    fn new(inventory: &'a Inventory) -> Self {
        Self {
            inventory,
            missing: Vec::new(),
        }
    }

    fn file(&mut self, folder: Folder, label: &str, test: impl Fn(&str) -> bool) -> String {
        let found = self
            .inventory
            .in_folder(folder)
            .iter()
            .find(|file| test(&base(file)))
            .cloned();
        found.unwrap_or_else(|| {
            self.missing.push(label.to_owned());
            String::new()
        })
    }

    fn any(&mut self, folder: Folder, label: &str, preferred: &str) -> String {
        prefer(self.inventory.in_folder(folder), preferred).unwrap_or_else(|| {
            self.missing.push(label.to_owned());
            String::new()
        })
    }

    fn nodes(&mut self, groups: &[&[&str]]) {
        let mut absent: Vec<String> = groups
            .iter()
            .flat_map(|group| group.iter())
            .filter(|node| !self.inventory.has_node(node))
            .map(|node| format!("вузол {node}"))
            .collect();
        absent.sort();
        absent.dedup();
        self.missing.extend(absent);
    }

    fn finish(self, id: String, name: String, note: String, medium: Medium, rank: u8, recipe: Recipe) -> Candidate {
        let ready = self.missing.is_empty();
        Candidate {
            id,
            name,
            note,
            precision: None,
            medium,
            rank,
            recipe: ready.then_some(recipe),
            missing: self.missing,
        }
    }
}

fn z_image(inventory: &Inventory, unet: &str, name: &str) -> Candidate {
    let mut needs = Needs::new(inventory);
    let clip = needs.file(Folder::TextEncoders, "qwen_3_4b (text encoder)", |file| {
        file.contains("qwen_3_4b") || file.contains("qwen3_4b") || file.contains("qwen3-4b")
    });
    let vae = needs.file(Folder::Vae, "ae.safetensors (VAE)", |file| {
        file.starts_with("ae.") || file.contains("z_image_vae") || file.contains("flux1_vae")
    });
    needs.nodes(&[&IMAGE_NODES, &Z_IMAGE_NODES]);
    let turbo = name.contains("turbo");
    needs.finish(
        format!("z-image:{unet}"),
        if turbo { "Z-Image Turbo" } else { "Z-Image" }.to_owned(),
        "Tongyi · фотореалізм і текст".to_owned(),
        Medium::Image,
        if turbo { 0 } else { 2 },
        Recipe::ZImage {
            unet: unet.to_owned(),
            clip,
            vae,
            turbo,
        },
    )
}

fn flux2(inventory: &Inventory, unet: &str, name: &str) -> Candidate {
    let mut needs = Needs::new(inventory);
    let large = name.contains("9b");
    let clip = needs.file(
        Folder::TextEncoders,
        if large { "qwen_3_8b (text encoder)" } else { "qwen_3_4b (text encoder)" },
        |file| {
            if large {
                file.contains("qwen_3_8b") || file.contains("qwen3_8b")
            } else {
                file.contains("qwen_3_4b") || file.contains("qwen3_4b")
            }
        },
    );
    let vae = needs.file(Folder::Vae, "flux2-vae (VAE)", |file| {
        file.contains("flux2") || file.contains("flux-2") || file.contains("flux_2")
    });
    needs.nodes(&[&IMAGE_NODES, &FLUX2_NODES]);
    let distilled = !name.contains("base");
    let size = if large { "9B" } else { "4B" };
    let label = if distilled {
        format!("FLUX.2 Klein {size}")
    } else {
        format!("FLUX.2 Klein {size} Base")
    };
    needs.finish(
        format!("flux2:{unet}"),
        label,
        "Black Forest Labs · швидкий".to_owned(),
        Medium::Image,
        1,
        Recipe::Flux2 {
            unet: unet.to_owned(),
            clip,
            vae,
            distilled,
        },
    )
}

fn checkpoint(inventory: &Inventory, file: &str, name: &str) -> Candidate {
    let mut needs = Needs::new(inventory);
    needs.nodes(&[&IMAGE_NODES, &CHECKPOINT_NODES]);
    let xl = XL_MARKERS.iter().any(|marker| name.contains(marker));
    let stem = name
        .rsplit_once('.')
        .map_or(name, |(stem, _)| stem)
        .replace(['_', '-'], " ");
    let title: String = stem.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(32).collect();
    needs.finish(
        format!("checkpoint:{file}"),
        title,
        if xl { "SDXL-чекпойнт" } else { "SD-чекпойнт" }.to_owned(),
        Medium::Image,
        if xl { 3 } else { 4 },
        Recipe::Checkpoint {
            file: file.to_owned(),
            xl,
        },
    )
}

fn ace_all_in_one(inventory: &Inventory, file: &str, name: &str) -> Candidate {
    let mut needs = Needs::new(inventory);
    needs.nodes(&[&ACE_NODES, &["CheckpointLoaderSimple"]]);
    let turbo = name.contains("turbo");
    needs.finish(
        format!("ace-aio:{file}"),
        if turbo { "ACE-Step 1.5 Turbo" } else { "ACE-Step 1.5" }.to_owned(),
        "Музика й звук · все в одному файлі".to_owned(),
        Medium::Audio,
        0,
        Recipe::AceAllInOne {
            file: file.to_owned(),
            turbo,
        },
    )
}

fn ace_split(inventory: &Inventory, unet: &str, name: &str) -> Candidate {
    let mut needs = Needs::new(inventory);
    let xl = name.contains("xl");
    let small = needs.file(Folder::TextEncoders, "qwen_0.6b_ace15 (text encoder)", |file| {
        file.contains("0.6b") && file.contains("ace")
    });
    let large = if xl {
        needs.file(Folder::TextEncoders, "qwen_4b_ace15 (text encoder)", |file| {
            file.contains("qwen_4b") && file.contains("ace")
        })
    } else {
        needs.file(Folder::TextEncoders, "qwen_1.7b_ace15 (text encoder)", |file| {
            file.contains("1.7b") && file.contains("ace")
        })
    };
    let vae = needs.file(Folder::Vae, "ace_1.5_vae (VAE)", |file| {
        file.contains("ace") && file.contains("vae")
    });
    needs.nodes(&[&ACE_NODES, &["UNETLoader", "DualCLIPLoader", "VAELoader"]]);
    let turbo = name.contains("turbo");
    let variant = if turbo {
        "Turbo"
    } else if name.contains("sft") {
        "SFT"
    } else {
        "Base"
    };
    let label = if xl {
        format!("ACE-Step 1.5 XL {variant}")
    } else {
        format!("ACE-Step 1.5 {variant}")
    };
    needs.finish(
        format!("ace:{unet}"),
        label,
        "Музика й звук".to_owned(),
        Medium::Audio,
        if xl { 1 } else { 2 },
        Recipe::AceSplit {
            unet: unet.to_owned(),
            small,
            large,
            vae,
            turbo,
        },
    )
}

fn stable_audio(inventory: &Inventory, file: &str) -> Candidate {
    let mut needs = Needs::new(inventory);
    let clip = needs.file(Folder::TextEncoders, "t5-base (text encoder)", |text| {
        text.contains("t5") && text.contains("base")
    });
    needs.nodes(&[&STABLE_AUDIO_NODES]);
    needs.finish(
        format!("stable-audio:{file}"),
        "Stable Audio Open".to_owned(),
        "Stability AI · SFX і лупи".to_owned(),
        Medium::Audio,
        3,
        Recipe::StableAudio {
            file: file.to_owned(),
            clip,
        },
    )
}

struct MeshParts {
    shape: String,
    texture: String,
    vision: String,
    matting: String,
}

fn mesh_parts(needs: &mut Needs<'_>) -> MeshParts {
    MeshParts {
        shape: needs.file(Folder::Vae, "trellis_2_shape_vae (VAE)", |file| {
            file.contains("trellis") && file.contains("shape")
        }),
        texture: needs.file(Folder::Vae, "trellis_2_texture_vae (VAE)", |file| {
            file.contains("trellis") && file.contains("texture")
        }),
        vision: needs.file(Folder::ClipVision, "dino_v3 (clip vision)", |file| file.contains("dino")),
        matting: needs.any(Folder::BackgroundRemoval, "birefnet (background removal)", "birefnet"),
    }
}

fn trellis(inventory: &Inventory, unet: &str) -> Candidate {
    let mut needs = Needs::new(inventory);
    let parts = mesh_parts(&mut needs);
    needs.nodes(&[&MESH_NODES, &TRELLIS_NODES]);
    needs.finish(
        format!("trellis:{unet}"),
        "TRELLIS.2".to_owned(),
        "Microsoft · картинка → 3D".to_owned(),
        Medium::Mesh,
        0,
        Recipe::Trellis {
            unet: unet.to_owned(),
            shape: parts.shape,
            texture: parts.texture,
            vision: parts.vision,
            matting: parts.matting,
        },
    )
}

fn pixal(inventory: &Inventory, unet: &str) -> Candidate {
    let mut needs = Needs::new(inventory);
    let parts = mesh_parts(&mut needs);
    let geometry = needs.any(Folder::Geometry, "moge_2 (geometry estimation)", "moge");
    needs.nodes(&[&MESH_NODES, &PIXAL_NODES]);
    needs.finish(
        format!("pixal3d:{unet}"),
        "Pixal3D".to_owned(),
        "Картинка → 3D з нормалями й AO".to_owned(),
        Medium::Mesh,
        1,
        Recipe::Pixal {
            unet: unet.to_owned(),
            shape: parts.shape,
            texture: parts.texture,
            vision: parts.vision,
            matting: parts.matting,
            geometry,
        },
    )
}

fn ultratex(inventory: &Inventory, lora: &str) -> Candidate {
    let mut needs = Needs::new(inventory);
    let unet = needs.file(Folder::DiffusionModels, "flux-2-klein-base-9b (diffusion model)", is_klein_base_9b);
    let clip = needs.file(Folder::TextEncoders, "qwen_3_8b (text encoder)", |file| {
        file.contains("qwen_3_8b") || file.contains("qwen3_8b")
    });
    let vae = needs.file(Folder::Vae, "flux2-vae (VAE)", |file| {
        file.contains("flux2") || file.contains("flux-2") || file.contains("flux_2")
    });
    let decoder = needs.file(Folder::UltraTex, "ultratex flux2 decoder.pt", |file| {
        file.ends_with(".pt") && (file.contains("ultratex") || file.contains("decoder")) && !file.contains("flux1")
    });
    let matting = needs.any(Folder::BackgroundRemoval, "birefnet (background removal)", "birefnet");
    needs.nodes(&[&ULTRATEX_NODES]);
    let precision = precision(&base(&unet)).map(str::to_owned);
    let mut candidate = needs.finish(
        format!("ultratex:{lora}"),
        "UltraTex".to_owned(),
        "Перетекстурує меш TRELLIS/Pixal3D · FLUX.2 9B".to_owned(),
        Medium::Mesh,
        2,
        Recipe::UltraTex {
            unet,
            clip,
            vae,
            lora: lora.to_owned(),
            decoder,
            matting,
        },
    );
    candidate.precision = precision;
    candidate
}

struct Graph {
    nodes: Map<String, Value>,
}

impl Graph {
    fn new() -> Self {
        Self { nodes: Map::new() }
    }

    fn add(&mut self, id: &str, class: &str, inputs: Value) {
        self.nodes.insert(
            id.to_owned(),
            json!({ "class_type": class, "inputs": inputs }),
        );
    }

    fn done(self) -> Value {
        Value::Object(self.nodes)
    }
}

fn link(id: &str, slot: u32) -> Value {
    json!([id, slot])
}

pub fn graph(recipe: &Recipe, brief: &Brief<'_>) -> Value {
    match recipe {
        Recipe::ZImage {
            unet,
            clip,
            vae,
            turbo,
        } => z_image_graph(unet, clip, vae, *turbo, brief),
        Recipe::Flux2 {
            unet,
            clip,
            vae,
            distilled,
        } => flux2_graph(unet, clip, vae, *distilled, brief),
        Recipe::Checkpoint { file, xl } => checkpoint_graph(file, *xl, brief),
        Recipe::AceAllInOne { file, turbo } => {
            let mut graph = Graph::new();
            graph.add("loader", "CheckpointLoaderSimple", json!({ "ckpt_name": file }));
            ace_tail(&mut graph, link("loader", 0), link("loader", 1), link("loader", 2), *turbo, brief);
            graph.done()
        }
        Recipe::AceSplit {
            unet,
            small,
            large,
            vae,
            turbo,
        } => {
            let mut graph = Graph::new();
            graph.add("unet", "UNETLoader", json!({ "unet_name": unet, "weight_dtype": "default" }));
            graph.add(
                "clip",
                "DualCLIPLoader",
                json!({ "clip_name1": small, "clip_name2": large, "type": "ace", "device": "default" }),
            );
            graph.add("vae", "VAELoader", json!({ "vae_name": vae }));
            ace_tail(&mut graph, link("unet", 0), link("clip", 0), link("vae", 0), *turbo, brief);
            graph.done()
        }
        Recipe::StableAudio { file, clip } => stable_audio_graph(file, clip, brief),
        Recipe::Trellis {
            unet,
            shape,
            texture,
            vision,
            matting,
        } => trellis_graph(unet, shape, texture, vision, matting, brief),
        Recipe::Pixal {
            unet,
            shape,
            texture,
            vision,
            matting,
            geometry,
        } => pixal_graph(unet, shape, texture, vision, matting, geometry, brief),
        Recipe::UltraTex {
            unet,
            clip,
            vae,
            lora,
            decoder,
            matting,
        } => ultratex_graph(unet, clip, vae, lora, decoder, matting, brief),
    }
}

fn image_prompt<'a>(brief: &'a Brief<'_>) -> &'a str {
    brief.prompt
}

fn negative_prompt(brief: &Brief<'_>) -> String {
    if brief.negative.trim().is_empty() {
        NEGATIVE_FALLBACK.to_owned()
    } else {
        brief.negative.to_owned()
    }
}

fn finish_image(graph: &mut Graph, image: Value, brief: &Brief<'_>) {
    let matting = brief.matting.filter(|_| brief.kind.transparent());
    let saved = match matting {
        Some(model) => {
            graph.add("matting", "LoadBackgroundRemovalModel", json!({ "bg_removal_name": model }));
            graph.add(
                "foreground",
                "RemoveBackground",
                json!({ "bg_removal_model": link("matting", 0), "image": image.clone() }),
            );
            graph.add("inverted", "InvertMask", json!({ "mask": link("foreground", 0) }));
            graph.add(
                "cutout",
                "JoinImageWithAlpha",
                json!({ "image": image, "alpha": link("inverted", 0) }),
            );
            link("cutout", 0)
        }
        None => image,
    };
    graph.add(
        "save",
        "SaveImage",
        json!({ "images": saved, "filename_prefix": brief.prefix }),
    );
}

fn z_image_graph(unet: &str, clip: &str, vae: &str, turbo: bool, brief: &Brief<'_>) -> Value {
    let (width, height) = brief.aspect.size(true);
    let mut graph = Graph::new();
    graph.add("unet", "UNETLoader", json!({ "unet_name": unet, "weight_dtype": "default" }));
    graph.add("clip", "CLIPLoader", json!({ "clip_name": clip, "type": "lumina2", "device": "default" }));
    graph.add("vae", "VAELoader", json!({ "vae_name": vae }));
    graph.add("sampling", "ModelSamplingAuraFlow", json!({ "model": link("unet", 0), "shift": 3.0 }));
    graph.add("positive", "CLIPTextEncode", json!({ "clip": link("clip", 0), "text": image_prompt(brief) }));
    if turbo {
        graph.add("negative", "ConditioningZeroOut", json!({ "conditioning": link("positive", 0) }));
    } else {
        graph.add("negative", "CLIPTextEncode", json!({ "clip": link("clip", 0), "text": negative_prompt(brief) }));
    }
    graph.add("latent", "EmptySD3LatentImage", json!({ "width": width, "height": height, "batch_size": 1 }));
    let (steps, cfg) = if turbo { (9, 1.0) } else { (28, 4.0) };
    graph.add(
        "sample",
        "KSampler",
        json!({
            "model": link("sampling", 0), "seed": brief.seed, "steps": steps, "cfg": cfg,
            "sampler_name": "res_multistep", "scheduler": "simple",
            "positive": link("positive", 0), "negative": link("negative", 0),
            "latent_image": link("latent", 0), "denoise": 1.0
        }),
    );
    graph.add("decode", "VAEDecode", json!({ "samples": link("sample", 0), "vae": link("vae", 0) }));
    finish_image(&mut graph, link("decode", 0), brief);
    graph.done()
}

fn flux2_graph(unet: &str, clip: &str, vae: &str, distilled: bool, brief: &Brief<'_>) -> Value {
    let (width, height) = brief.aspect.size(true);
    let mut graph = Graph::new();
    graph.add("unet", "UNETLoader", json!({ "unet_name": unet, "weight_dtype": "default" }));
    graph.add("clip", "CLIPLoader", json!({ "clip_name": clip, "type": "flux2", "device": "default" }));
    graph.add("vae", "VAELoader", json!({ "vae_name": vae }));
    graph.add("latent", "EmptyFlux2LatentImage", json!({ "width": width, "height": height, "batch_size": 1 }));
    graph.add("sampler", "KSamplerSelect", json!({ "sampler_name": "euler" }));
    let (steps, cfg) = if distilled { (4, 1.0) } else { (28, 4.0) };
    graph.add("sigmas", "Flux2Scheduler", json!({ "steps": steps, "width": width, "height": height }));
    graph.add("positive", "CLIPTextEncode", json!({ "clip": link("clip", 0), "text": image_prompt(brief) }));
    if distilled {
        graph.add("negative", "ConditioningZeroOut", json!({ "conditioning": link("positive", 0) }));
    } else {
        graph.add("negative", "CLIPTextEncode", json!({ "clip": link("clip", 0), "text": negative_prompt(brief) }));
    }
    graph.add(
        "guider",
        "CFGGuider",
        json!({ "model": link("unet", 0), "positive": link("positive", 0), "negative": link("negative", 0), "cfg": cfg }),
    );
    graph.add("noise", "RandomNoise", json!({ "noise_seed": brief.seed }));
    graph.add(
        "sample",
        "SamplerCustomAdvanced",
        json!({
            "noise": link("noise", 0), "guider": link("guider", 0), "sampler": link("sampler", 0),
            "sigmas": link("sigmas", 0), "latent_image": link("latent", 0)
        }),
    );
    graph.add("decode", "VAEDecode", json!({ "samples": link("sample", 0), "vae": link("vae", 0) }));
    finish_image(&mut graph, link("decode", 0), brief);
    graph.done()
}

fn checkpoint_graph(file: &str, xl: bool, brief: &Brief<'_>) -> Value {
    let (width, height) = brief.aspect.size(xl);
    let text = if brief.tags.trim().is_empty() {
        brief.prompt
    } else {
        brief.tags
    };
    let mut graph = Graph::new();
    graph.add("loader", "CheckpointLoaderSimple", json!({ "ckpt_name": file }));
    graph.add("positive", "CLIPTextEncode", json!({ "clip": link("loader", 1), "text": text }));
    graph.add("negative", "CLIPTextEncode", json!({ "clip": link("loader", 1), "text": negative_prompt(brief) }));
    graph.add("latent", "EmptyLatentImage", json!({ "width": width, "height": height, "batch_size": 1 }));
    graph.add(
        "sample",
        "KSampler",
        json!({
            "model": link("loader", 0), "seed": brief.seed, "steps": 28, "cfg": 6.0,
            "sampler_name": "dpmpp_2m", "scheduler": "karras",
            "positive": link("positive", 0), "negative": link("negative", 0),
            "latent_image": link("latent", 0), "denoise": 1.0
        }),
    );
    graph.add("decode", "VAEDecode", json!({ "samples": link("sample", 0), "vae": link("loader", 2) }));
    finish_image(&mut graph, link("decode", 0), brief);
    graph.done()
}

fn ace_tail(graph: &mut Graph, model: Value, clip: Value, vae: Value, turbo: bool, brief: &Brief<'_>) {
    let tags = if brief.tags.trim().is_empty() {
        brief.prompt
    } else {
        brief.tags
    };
    let lyrics = if brief.lyrics.trim().is_empty() {
        "[Instrumental]"
    } else {
        brief.lyrics
    };
    let keyscale = if valid_keyscale(brief.keyscale) {
        brief.keyscale
    } else {
        DEFAULT_KEYSCALE
    };
    graph.add("sampling", "ModelSamplingAuraFlow", json!({ "model": model, "shift": 3.0 }));
    graph.add(
        "positive",
        "TextEncodeAceStepAudio1.5",
        json!({
            "clip": clip, "tags": tags, "lyrics": lyrics, "seed": brief.seed,
            "bpm": brief.bpm, "duration": brief.seconds, "timesignature": "4",
            "language": "en", "keyscale": keyscale, "generate_audio_codes": true,
            "cfg_scale": 2.0, "temperature": 0.85, "top_p": 0.9, "top_k": 0, "min_p": 0.0
        }),
    );
    graph.add("negative", "ConditioningZeroOut", json!({ "conditioning": link("positive", 0) }));
    graph.add("latent", "EmptyAceStep1.5LatentAudio", json!({ "seconds": brief.seconds, "batch_size": 1 }));
    let (steps, cfg) = if turbo { (8, 1.0) } else { (50, 7.0) };
    graph.add(
        "sample",
        "KSampler",
        json!({
            "model": link("sampling", 0), "seed": brief.seed, "steps": steps, "cfg": cfg,
            "sampler_name": "euler", "scheduler": "simple",
            "positive": link("positive", 0), "negative": link("negative", 0),
            "latent_image": link("latent", 0), "denoise": 1.0
        }),
    );
    graph.add("decode", "VAEDecodeAudio", json!({ "samples": link("sample", 0), "vae": vae }));
    graph.add(
        "save",
        "SaveAudioAdvanced",
        json!({ "audio": link("decode", 0), "filename_prefix": brief.prefix, "format": "flac" }),
    );
}

pub fn valid_keyscale(value: &str) -> bool {
    const ROOTS: [&str; 17] = [
        "C", "C#", "Db", "D", "D#", "Eb", "E", "F", "F#", "Gb", "G", "G#", "Ab", "A", "A#", "Bb", "B",
    ];
    value
        .split_once(' ')
        .is_some_and(|(root, quality)| ROOTS.contains(&root) && matches!(quality, "major" | "minor"))
}

fn stable_audio_graph(file: &str, clip: &str, brief: &Brief<'_>) -> Value {
    let text = if brief.tags.trim().is_empty() {
        brief.prompt
    } else {
        brief.tags
    };
    let mut graph = Graph::new();
    graph.add("loader", "CheckpointLoaderSimple", json!({ "ckpt_name": file }));
    graph.add("clip", "CLIPLoader", json!({ "clip_name": clip, "type": "stable_audio", "device": "default" }));
    graph.add("positive", "CLIPTextEncode", json!({ "clip": link("clip", 0), "text": text }));
    graph.add("negative", "CLIPTextEncode", json!({ "clip": link("clip", 0), "text": "music, vocals, speech, noise, distortion, low quality" }));
    graph.add("latent", "EmptyLatentAudio", json!({ "seconds": brief.seconds, "batch_size": 1 }));
    graph.add(
        "sample",
        "KSampler",
        json!({
            "model": link("loader", 0), "seed": brief.seed, "steps": 50, "cfg": 4.98,
            "sampler_name": "dpmpp_3m_sde_gpu", "scheduler": "exponential",
            "positive": link("positive", 0), "negative": link("negative", 0),
            "latent_image": link("latent", 0), "denoise": 1.0
        }),
    );
    graph.add("decode", "VAEDecodeAudio", json!({ "samples": link("sample", 0), "vae": link("loader", 2) }));
    graph.add(
        "save",
        "SaveAudioAdvanced",
        json!({ "audio": link("decode", 0), "filename_prefix": brief.prefix, "format": "flac" }),
    );
    graph.done()
}

struct MeshStart<'a> {
    unet: &'a str,
    shape: &'a str,
    texture: &'a str,
    vision: &'a str,
    matting: &'a str,
    pad: f64,
    background: &'a str,
}

fn mesh_start(graph: &mut Graph, parts: &MeshStart<'_>, brief: &Brief<'_>) {
    graph.add("image", "LoadImage", json!({ "image": brief.reference.unwrap_or_default() }));
    graph.add("matting", "LoadBackgroundRemovalModel", json!({ "bg_removal_name": parts.matting }));
    graph.add(
        "mask",
        "RemoveBackground",
        json!({ "bg_removal_model": link("matting", 0), "image": link("image", 0) }),
    );
    graph.add(
        "crop",
        "ImageCropToMask",
        json!({
            "images": link("image", 0), "masks": link("mask", 0), "width": 1024, "height": 1024,
            "pad_factor": parts.pad, "grow_mask": 0, "background": parts.background
        }),
    );
    graph.add("vision", "CLIPVisionLoader", json!({ "clip_name": parts.vision }));
    graph.add("unet", "UNETLoader", json!({ "unet_name": parts.unet, "weight_dtype": "default" }));
    graph.add("shape_vae", "VAELoader", json!({ "vae_name": parts.shape }));
    graph.add("texture_vae", "VAELoader", json!({ "vae_name": parts.texture }));
    graph.add(
        "cfg_structure",
        "CFGOverride",
        json!({ "model": link("unet", 0), "cfg": 1.0, "start_percent": 0.667, "end_percent": 1.0 }),
    );
    graph.add("rescale_structure", "RescaleCFG", json!({ "model": link("cfg_structure", 0), "multiplier": 0.7 }));
    graph.add("sampling_structure", "ModelSamplingSD3", json!({ "model": link("rescale_structure", 0), "shift": 5.0 }));
    graph.add(
        "cfg_shape",
        "CFGOverride",
        json!({ "model": link("unet", 0), "cfg": 1.0, "start_percent": 0.769, "end_percent": 1.0 }),
    );
    graph.add("rescale_shape", "RescaleCFG", json!({ "model": link("cfg_shape", 0), "multiplier": 0.5 }));
    graph.add("empty", "EmptyTrellis2LatentStructure", json!({ "batch_size": 1 }));
}

fn mesh_sampling(graph: &mut Graph, upsample_resolution: u32, brief: &Brief<'_>) {
    let sampler = |model: &str, positive: Value, negative: Value, latent: Value, steps: u32, cfg: f64, scheduler: &str, seed: u64| {
        json!({
            "model": link(model, 0), "seed": seed, "steps": steps, "cfg": cfg,
            "sampler_name": "euler", "scheduler": scheduler,
            "positive": positive, "negative": negative, "latent_image": latent, "denoise": 1.0
        })
    };
    graph.add(
        "structure",
        "KSampler",
        sampler("sampling_structure", link("cond", 0), link("cond", 1), link("empty", 0), 12, 7.5, "normal", brief.seed),
    );
    graph.add(
        "voxel",
        "VaeDecodeStructureTrellis2",
        json!({ "samples": link("structure", 0), "vae": link("shape_vae", 0), "resolution": "32" }),
    );
    graph.add(
        "shape_stage",
        "Trellis2ShapeStage",
        json!({ "positive": link("cond", 0), "negative": link("cond", 1), "voxel": link("voxel", 0) }),
    );
    graph.add(
        "shape",
        "KSampler",
        sampler("rescale_shape", link("shape_stage", 0), link("shape_stage", 1), link("shape_stage", 2), 20, 7.5, "normal", brief.seed),
    );
    graph.add(
        "upsample_stage",
        "Trellis2UpsampleStage",
        json!({
            "positive": link("shape_stage", 0), "negative": link("shape_stage", 1),
            "shape_latent": link("shape", 0), "vae": link("shape_vae", 0),
            "target_resolution": upsample_resolution
        }),
    );
    graph.add(
        "upsample",
        "KSampler",
        sampler("rescale_shape", link("upsample_stage", 0), link("upsample_stage", 1), link("upsample_stage", 2), 12, 7.5, "simple", brief.seed),
    );
    graph.add("dense", "VaeDecodeShapeTrellis", json!({ "samples": link("upsample", 0), "vae": link("shape_vae", 0) }));
    graph.add(
        "texture_stage",
        "Trellis2TextureStage",
        json!({ "positive": link("upsample_stage", 0), "negative": link("upsample_stage", 1), "shape_latent": link("upsample", 0) }),
    );
    graph.add(
        "texture",
        "KSampler",
        sampler("unet", link("texture_stage", 0), link("texture_stage", 1), link("texture_stage", 2), 12, 1.0, "normal", brief.seed + 1),
    );
    graph.add(
        "colors",
        "VaeDecodeTextureTrellis",
        json!({ "samples": link("texture", 0), "vae": link("texture_vae", 0), "shape_subdivides": link("dense", 1) }),
    );
}

fn mesh_bake(graph: &mut Graph, source: Value) {
    graph.add(
        "decimate",
        "DecimateMesh",
        json!({ "mesh": source, "target_face_count": MESH_FACES, "placement_mode": "midpoint" }),
    );
    graph.add("smooth", "MeshSmoothNormals", json!({ "mesh": link("decimate", 0), "crease_angle": 180.0 }));
    graph.add(
        "unwrap",
        "UnwrapMesh",
        json!({ "mesh": link("smooth", 0), "segmenter": "pec", "resolution": MESH_TEXTURE, "padding": 2, "weld_distance": 0.0 }),
    );
    graph.add(
        "bake",
        "BakeTextureFromVoxel",
        json!({ "mesh": link("unwrap", 0), "voxel_colors": link("colors", 0), "reference_mesh": link("dense", 0), "texture_size": MESH_TEXTURE }),
    );
}

fn trellis_graph(unet: &str, shape: &str, texture: &str, vision: &str, matting: &str, brief: &Brief<'_>) -> Value {
    let mut graph = Graph::new();
    let parts = MeshStart {
        unet,
        shape,
        texture,
        vision,
        matting,
        pad: 1.0,
        background: "#808080",
    };
    mesh_start(&mut graph, &parts, brief);
    graph.add(
        "cond",
        "Trellis2Conditioning",
        json!({ "clip_vision_model": link("vision", 0), "image": link("crop", 0) }),
    );
    mesh_sampling(&mut graph, 1024, brief);
    mesh_bake(&mut graph, link("dense", 0));
    graph.add(
        "apply",
        "ApplyTextureToMesh",
        json!({ "mesh": link("unwrap", 0), "base_color": link("bake", 0), "metallic": link("bake", 1), "roughness": link("bake", 2) }),
    );
    graph.add("save", "SaveGLB", json!({ "mesh": link("apply", 0), "filename_prefix": brief.prefix }));
    graph.done()
}

fn pixal_graph(
    unet: &str,
    shape: &str,
    texture: &str,
    vision: &str,
    matting: &str,
    geometry: &str,
    brief: &Brief<'_>,
) -> Value {
    let mut graph = Graph::new();
    let parts = MeshStart {
        unet,
        shape,
        texture,
        vision,
        matting,
        pad: 1.1,
        background: "#000000",
    };
    mesh_start(&mut graph, &parts, brief);
    graph.add("geometry_model", "LoadMoGeModel", json!({ "model_name": geometry }));
    graph.add(
        "geometry",
        "MoGeInference",
        json!({
            "moge_model": link("geometry_model", 0), "image": link("crop", 0), "resolution_level": 9,
            "fov_x_degrees": 0.0, "batch_size": 4, "force_projection": true, "apply_mask": true, "refine_steps": 3
        }),
    );
    graph.add(
        "fov",
        "MoGeGeometryToFOV",
        json!({ "moge_geometry": link("geometry", 0), "axis": "horizontal", "unit": "degrees" }),
    );
    graph.add(
        "cond",
        "Pixal3DConditioning",
        json!({ "clip_vision_model": link("vision", 0), "image": link("crop", 0), "camera_angle_x": link("fov", 0) }),
    );
    mesh_sampling(&mut graph, 1536, brief);
    graph.add(
        "remesh",
        "RemeshMesh",
        json!({
            "mesh": link("dense", 0), "resolution": 1024, "sign_mode": "udf", "sign_mode.qef": false,
            "sign_mode.drop_inverted_components": false, "sign_mode.drop_enclosed_components": false,
            "band": 1.0, "project_back": 0.0, "fix_poles": true, "smooth_iters": 2,
            "drop_small_components": 0.0, "precluster_max_verts": 20_000_000
        }),
    );
    mesh_bake(&mut graph, link("remesh", 0));
    graph.add(
        "normal",
        "BakeNormalMapFromMesh",
        json!({ "low_poly": link("unwrap", 0), "high_poly": link("remesh", 0), "resolution": MESH_TEXTURE, "cage_distance": 0.05, "ignore_backfaces": true }),
    );
    graph.add(
        "occlusion",
        "BakeAmbientOcclusion",
        json!({
            "low_poly": link("unwrap", 0), "high_poly": link("remesh", 0), "resolution": 1024,
            "samples": 64, "max_distance": 0.71, "strength": 1.0, "bias": 0.01
        }),
    );
    graph.add(
        "apply",
        "ApplyTextureToMesh",
        json!({
            "mesh": link("unwrap", 0), "base_color": link("bake", 0), "metallic": link("bake", 1),
            "roughness": link("bake", 2), "occlusion": link("occlusion", 0), "normal_map": link("normal", 0)
        }),
    );
    graph.add("final", "MeshSmoothNormals", json!({ "mesh": link("apply", 0), "crease_angle": 180.0 }));
    graph.add("save", "SaveGLB", json!({ "mesh": link("final", 0), "filename_prefix": brief.prefix }));
    graph.done()
}

fn ultratex_graph(
    unet: &str,
    clip: &str,
    vae: &str,
    lora: &str,
    decoder: &str,
    matting: &str,
    brief: &Brief<'_>,
) -> Value {
    let mut graph = Graph::new();
    graph.add("image", "LoadImage", json!({ "image": brief.reference.unwrap_or_default() }));
    graph.add("matting", "LoadBackgroundRemovalModel", json!({ "bg_removal_name": matting }));
    graph.add(
        "mask",
        "RemoveBackground",
        json!({ "bg_removal_model": link("matting", 0), "image": link("image", 0) }),
    );
    graph.add("unet", "UNETLoader", json!({ "unet_name": unet, "weight_dtype": "default" }));
    graph.add(
        "lora",
        "UltraTexLoraLoader",
        json!({ "model": link("unet", 0), "lora_name": lora, "strength": 1.0 }),
    );
    graph.add("clip", "CLIPLoader", json!({ "clip_name": clip, "type": "flux2", "device": "default" }));
    graph.add("positive", "CLIPTextEncode", json!({ "clip": link("clip", 0), "text": "" }));
    graph.add("vae", "VAELoader", json!({ "vae_name": vae }));
    graph.add(
        "decoder",
        "UltraTexForegroundDecoder",
        json!({ "vae": link("vae", 0), "decoder_name": decoder }),
    );
    graph.add(
        "prep",
        "UltraTexPrep",
        json!({
            "reference_image": link("image", 0), "mesh_path": brief.mesh.unwrap_or_default(),
            "render_size": ULTRATEX_RENDER_SIZE, "atlas_size": MESH_TEXTURE, "uv_mode": "comfy_gpu",
            "reference_alignment": "fit_front_silhouette", "mask_meaning": "foreground",
            "reference_mask": link("mask", 0)
        }),
    );
    graph.add(
        "sample",
        "UltraTexSampler",
        json!({
            "model": link("lora", 0), "vae": link("decoder", 0), "positive": link("positive", 0),
            "prep": link("prep", 0), "resolution": ULTRATEX_VIEW_SIZE, "steps": ULTRATEX_STEPS,
            "seed": brief.seed, "guidance": ULTRATEX_GUIDANCE, "drop_background_tokens": true
        }),
    );
    graph.add(
        "save",
        "UltraTexBake",
        json!({
            "prep": link("prep", 0), "albedo_views": link("sample", 0), "texture_size": MESH_TEXTURE,
            "filename_prefix": brief.prefix, "view_weight_power": 4.0, "best_view_mix": 0.5,
            "edge_feather_px": 6.0
        }),
    );
    graph.done()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn this_machine() -> Inventory {
        let mut files = BTreeMap::new();
        files.insert(Folder::Checkpoints, vec!["ace_step_1.5_turbo_aio.safetensors".to_owned()]);
        files.insert(
            Folder::DiffusionModels,
            vec![
                "acestep_v1.5_xl_sft_bf16.safetensors".to_owned(),
                "flux-2-klein-4b-fp8.safetensors".to_owned(),
                "pixal3d_int8_convrot.safetensors".to_owned(),
                "trellis_2_int8_convrot.safetensors".to_owned(),
                "z_image_turbo_int8_convrot.safetensors".to_owned(),
            ],
        );
        files.insert(
            Folder::TextEncoders,
            vec![
                "qwen_0.6b_ace15.safetensors".to_owned(),
                "qwen_3_4b_fp4_mixed.safetensors".to_owned(),
                "qwen_4b_ace15.safetensors".to_owned(),
                "t5-base.safetensors".to_owned(),
            ],
        );
        files.insert(
            Folder::Vae,
            vec![
                "ace_1.5_vae.safetensors".to_owned(),
                "ae.safetensors".to_owned(),
                "flux2-vae.safetensors".to_owned(),
                "trellis_2_shape_vae_bf16.safetensors".to_owned(),
                "trellis_2_texture_vae_bf16.safetensors".to_owned(),
            ],
        );
        files.insert(Folder::ClipVision, vec!["dino_v3_L_naf_fp32.safetensors".to_owned()]);
        files.insert(Folder::BackgroundRemoval, vec!["birefnet.safetensors".to_owned()]);
        files.insert(Folder::Geometry, vec!["moge_2_vitl_normal_fp16.safetensors".to_owned()]);
        Inventory { files, nodes: None }
    }

    #[test]
    fn finds_every_model_installed_here() {
        let roster = roster(&this_machine());
        let names: Vec<(&str, Medium, bool)> = roster
            .iter()
            .map(|candidate| (candidate.name.as_str(), candidate.medium, candidate.ready()))
            .collect();
        assert_eq!(
            names,
            vec![
                ("Z-Image Turbo", Medium::Image, true),
                ("FLUX.2 Klein 4B", Medium::Image, true),
                ("TRELLIS.2", Medium::Mesh, true),
                ("Pixal3D", Medium::Mesh, true),
                ("ACE-Step 1.5 Turbo", Medium::Audio, true),
                ("ACE-Step 1.5 XL SFT", Medium::Audio, true),
            ]
        );
        let precisions: Vec<Option<&str>> = roster
            .iter()
            .map(|candidate| candidate.precision.as_deref())
            .collect();
        assert_eq!(
            precisions,
            vec![Some("int8"), Some("fp8"), Some("int8"), Some("int8"), None, Some("bf16")]
        );
        assert_eq!(roster[0].note, "Tongyi · фотореалізм і текст");
    }

    #[test]
    fn reports_what_is_missing() {
        let mut inventory = this_machine();
        inventory.files.remove(&Folder::Geometry);
        inventory.nodes = Some(["UNETLoader".to_owned()].into_iter().collect());
        let roster = roster(&inventory);
        let pixal = roster.iter().find(|candidate| candidate.name == "Pixal3D").unwrap();
        assert!(!pixal.ready());
        assert!(pixal.missing.contains(&"moge_2 (geometry estimation)".to_owned()));
        assert!(pixal.missing.contains(&"вузол Pixal3DConditioning".to_owned()));
    }

    #[test]
    fn generic_checkpoints_join_the_image_roster() {
        let mut inventory = this_machine();
        inventory.files.get_mut(&Folder::Checkpoints).unwrap().extend([
            "sd_xl_base_1.0.safetensors".to_owned(),
            "ltx-video-2b.safetensors".to_owned(),
        ]);
        let roster = roster(&inventory);
        let generic: Vec<&str> = roster
            .iter()
            .filter(|candidate| candidate.id.starts_with("checkpoint:"))
            .map(|candidate| candidate.name.as_str())
            .collect();
        assert_eq!(generic, vec!["sd xl base 1.0"]);
    }

    fn brief(kind: AssetKind, matting: Option<&'static str>, reference: Option<&'static str>) -> Brief<'static> {
        Brief {
            kind,
            prompt: "a glowing fire sword",
            tags: "fire sword, glowing",
            negative: "",
            aspect: Aspect::Portrait,
            seconds: 4.0,
            bpm: 120,
            keyscale: "Z major",
            lyrics: "",
            seed: 77,
            prefix: "noctics/1/z_image",
            matting,
            reference,
            mesh: None,
        }
    }

    fn with_ultratex(mut inventory: Inventory) -> Inventory {
        inventory
            .files
            .get_mut(&Folder::DiffusionModels)
            .unwrap()
            .push("flux-2-klein-base-9b-fp8.safetensors".to_owned());
        inventory
            .files
            .get_mut(&Folder::TextEncoders)
            .unwrap()
            .push("qwen_3_8b_fp4mixed.safetensors".to_owned());
        inventory.files.insert(
            Folder::Loras,
            vec!["ultratex_flux2.safetensors".to_owned(), "ultratex_flux2_mr.safetensors".to_owned()],
        );
        inventory.files.insert(
            Folder::UltraTex,
            vec!["flux2-vae.safetensors".to_owned(), "ultratex_flux2_decoder.pt".to_owned()],
        );
        inventory
    }

    #[test]
    fn ultratex_retextures_the_mesh_of_another_model() {
        let roster = roster(&with_ultratex(this_machine()));
        let meshes: Vec<&str> = roster
            .iter()
            .filter(|candidate| candidate.medium == Medium::Mesh)
            .map(|candidate| candidate.name.as_str())
            .collect();
        assert_eq!(meshes, vec!["TRELLIS.2", "Pixal3D", "UltraTex"]);
        assert!(roster.iter().any(|candidate| candidate.name == "FLUX.2 Klein 9B Base" && candidate.ready()));
        let ultratex = roster.iter().find(|candidate| candidate.name == "UltraTex").unwrap();
        assert!(ultratex.ready(), "{:?}", ultratex.missing);
        assert_eq!(ultratex.precision.as_deref(), Some("fp8"));
        let recipe = ultratex.recipe.as_ref().unwrap();
        assert!(recipe.needs_mesh());
        let mut request = brief(AssetKind::Model, Some("birefnet.safetensors"), Some("noctics/1/ref_00001_.png [output]"));
        request.mesh = Some("noctics/1/2-0_00001_.glb");
        let value = graph(recipe, &request);
        links_resolve(&value);
        assert_eq!(value["prep"]["inputs"]["mesh_path"], "noctics/1/2-0_00001_.glb");
        assert_eq!(value["prep"]["inputs"]["mask_meaning"], "foreground");
        assert_eq!(value["lora"]["inputs"]["lora_name"], "ultratex_flux2.safetensors");
        assert_eq!(value["decoder"]["inputs"]["decoder_name"], "ultratex_flux2_decoder.pt");
        assert_eq!(value["clip"]["inputs"]["clip_name"], "qwen_3_8b_fp4mixed.safetensors");
        assert_eq!(value["save"]["class_type"], "UltraTexBake");
    }

    #[test]
    fn ultratex_names_what_it_still_needs() {
        let mut inventory = this_machine();
        inventory
            .files
            .insert(Folder::Loras, vec!["ultratex_flux2.safetensors".to_owned()]);
        let roster = roster(&inventory);
        let ultratex = roster.iter().find(|candidate| candidate.name == "UltraTex").unwrap();
        assert!(!ultratex.ready());
        assert_eq!(
            ultratex.missing,
            vec![
                "flux-2-klein-base-9b (diffusion model)",
                "qwen_3_8b (text encoder)",
                "ultratex flux2 decoder.pt"
            ]
        );
    }

    fn classes(graph: &Value) -> Vec<String> {
        let mut found: Vec<String> = graph
            .as_object()
            .unwrap()
            .values()
            .map(|node| node["class_type"].as_str().unwrap().to_owned())
            .collect();
        found.sort();
        found
    }

    fn links_resolve(graph: &Value) {
        let nodes = graph.as_object().unwrap();
        for node in nodes.values() {
            for value in node["inputs"].as_object().unwrap().values() {
                if let Some([Value::String(target), Value::Number(_)]) = value.as_array().map(Vec::as_slice) {
                    assert!(nodes.contains_key(target), "dangling link to {target}");
                }
            }
        }
    }

    #[test]
    fn sprites_get_a_transparent_cutout() {
        let roster = roster(&this_machine());
        let recipe = roster[0].recipe.as_ref().unwrap();
        let graph = graph(recipe, &brief(AssetKind::Sprite, Some("birefnet.safetensors"), None));
        links_resolve(&graph);
        assert!(classes(&graph).contains(&"JoinImageWithAlpha".to_owned()));
        assert_eq!(graph["latent"]["inputs"]["width"], 832);
        assert_eq!(graph["save"]["inputs"]["images"], json!(["cutout", 0]));

        let concept = graph_for(&roster[1], AssetKind::Concept);
        assert!(!classes(&concept).contains(&"JoinImageWithAlpha".to_owned()));
        assert_eq!(concept["sigmas"]["inputs"]["steps"], 4);
    }

    fn graph_for(candidate: &Candidate, kind: AssetKind) -> Value {
        let value = graph(
            candidate.recipe.as_ref().unwrap(),
            &brief(kind, Some("birefnet.safetensors"), Some("noctics/1/ref_00001_.png [output]")),
        );
        links_resolve(&value);
        value
    }

    #[test]
    fn every_recipe_builds_a_connected_graph() {
        let roster = roster(&this_machine());
        for candidate in &roster {
            let kind = match candidate.medium {
                Medium::Image => AssetKind::Concept,
                Medium::Mesh => AssetKind::Model,
                Medium::Audio => AssetKind::Sound,
            };
            let value = graph_for(candidate, kind);
            assert!(value["save"].is_object(), "{} has no save node", candidate.name);
        }
        let audio = graph_for(&roster[5], AssetKind::Music);
        assert_eq!(audio["positive"]["inputs"]["keyscale"], DEFAULT_KEYSCALE);
        assert_eq!(audio["sample"]["inputs"]["steps"], 50);
        let mesh = graph_for(&roster[3], AssetKind::Model);
        assert_eq!(mesh["image"]["inputs"]["image"], "noctics/1/ref_00001_.png [output]");
    }

    #[test]
    fn validates_keyscales() {
        assert!(valid_keyscale("F# minor"));
        assert!(valid_keyscale("Bb major"));
        assert!(!valid_keyscale("H minor"));
        assert!(!valid_keyscale("C dorian"));
    }
}
