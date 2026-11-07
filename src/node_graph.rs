use noctics_shared::{
    AssetHunt, AssetTask, Build, BuildOverview, BuildStatus, BuildTarget, ComfyOverview, DESIGN_STAGE, GameEngine,
    HuntStatus, Medium, OutputStatus, PinterestStatus, PlaytestStatus, PlaytestVerdict, ProjectDetail, ProjectStatus,
    Readiness, Reference, RunStatus, ScreenStatus, SkillOutput, SourceStatus, TaskStatus, format_size,
};

use noctics_shared::{MixamoOverview, MotionOrder, MotionStatus, engine_takes_motions};
use noctics_shared::{DesignRunStatus, OpenDesignBoard};

use crate::asset_hunt::{HUNT_NOTE, HUNT_TITLE, found_summary, searching_note};
use crate::open_design::{VARIANT_WORDS, status_label, variants_summary};
use crate::journal::plural;
use crate::studio::explain;

const COLUMN: f64 = 310.0;
const NODE_WIDTH: f64 = 250.0;
const WIDE_NODE_WIDTH: f64 = 270.0;
const BUILD_SPACING: f64 = 178.0;
const VISIBLE_ROWS: usize = 5;
const VISIBLE_IMAGES: usize = 6;
const VISIBLE_LOG_LINES: usize = 3;
const VISIBLE_FRAMES: usize = 4;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Phase {
    Idle,
    Done,
    Review,
    Queued,
    Failed,
    Working,
}

impl Phase {
    pub fn code(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Done => "done",
            Self::Review => "review",
            Self::Queued => "queued",
            Self::Failed => "failed",
            Self::Working => "working",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "Очікує",
            Self::Done => "Готово",
            Self::Review => "Чекає на тебе",
            Self::Queued => "У черзі",
            Self::Failed => "Помилка",
            Self::Working => "Працює",
        }
    }

    fn urgency(self) -> u8 {
        match self {
            Self::Working => 4,
            Self::Failed => 3,
            Self::Review => 2,
            Self::Queued => 1,
            Self::Idle | Self::Done => 0,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cable {
    Brief,
    Visual,
    Code,
    Package,
}

impl Cable {
    pub const ALL: [Self; 4] = [Self::Brief, Self::Visual, Self::Code, Self::Package];

    pub fn class(self) -> &'static str {
        match self {
            Self::Brief => "cable-brief",
            Self::Visual => "cable-visual",
            Self::Code => "cable-code",
            Self::Package => "cable-package",
        }
    }

    pub fn tint(self) -> &'static str {
        match self {
            Self::Brief => "tint-brief",
            Self::Visual => "tint-visual",
            Self::Code => "tint-code",
            Self::Package => "tint-package",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Brief => "Тексти й документи",
            Self::Visual => "Стиль, референси, арт",
            Self::Code => "Код гри",
            Self::Package => "Готові білди",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum NodeId {
    Prompt,
    Plan,
    References,
    Foundation,
    Skills,
    Screens,
    Code,
    Assets,
    Finds,
    Animations,
    OpenDesign,
    Playtest,
    Builds,
    Build(BuildTarget),
}

impl NodeId {
    pub fn key(self) -> String {
        match self {
            Self::Prompt => "prompt".to_owned(),
            Self::Plan => "plan".to_owned(),
            Self::References => "references".to_owned(),
            Self::Foundation => "foundation".to_owned(),
            Self::Skills => "skills".to_owned(),
            Self::Screens => "screens".to_owned(),
            Self::Code => "code".to_owned(),
            Self::Assets => "assets".to_owned(),
            Self::Finds => "finds".to_owned(),
            Self::Animations => "animations".to_owned(),
            Self::OpenDesign => "open-design".to_owned(),
            Self::Playtest => "playtest".to_owned(),
            Self::Builds => "builds".to_owned(),
            Self::Build(target) => format!("build-{}", target.code()),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Port {
    pub label: &'static str,
    pub cable: Cable,
}

const IDEA: Port = Port {
    label: "ідея",
    cable: Cable::Brief,
};
const DOCUMENT: Port = Port {
    label: "документ",
    cable: Cable::Brief,
};
const STYLE: Port = Port {
    label: "стиль",
    cable: Cable::Visual,
};
const BOARD: Port = Port {
    label: "дошка",
    cable: Cable::Visual,
};
const TEXTS: Port = Port {
    label: "тексти",
    cable: Cable::Brief,
};
const SCREENS: Port = Port {
    label: "екрани",
    cable: Cable::Visual,
};
const SOURCE: Port = Port {
    label: "код",
    cable: Cable::Code,
};
const ORDERS: Port = Port {
    label: "замовлення",
    cable: Cable::Code,
};
const FILES: Port = Port {
    label: "ассети",
    cable: Cable::Visual,
};
const PACKAGE: Port = Port {
    label: "пакет",
    cable: Cable::Package,
};
const FOUND: Port = Port {
    label: "знахідки",
    cable: Cable::Visual,
};
const MOTIONS: Port = Port {
    label: "анімації",
    cable: Cable::Visual,
};
const VARIANTS: Port = Port {
    label: "варіації",
    cable: Cable::Visual,
};
const TESTED: Port = Port {
    label: "перевірено",
    cable: Cable::Code,
};

#[derive(Clone, PartialEq, Debug)]
pub struct Row {
    pub phase: Phase,
    pub text: String,
    pub tag: Option<String>,
}

#[derive(Clone, PartialEq, Debug)]
pub enum Body {
    Prompt { text: String, chips: Vec<String> },
    Plan { name: String, pitch: String, palette: Vec<String>, chips: Vec<String> },
    Gallery { images: Vec<String> },
    Frames { images: Vec<String> },
    Rows { rows: Vec<Row>, hidden: usize },
    Log { lines: Vec<String> },
    Empty,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Node {
    pub id: NodeId,
    pub title: String,
    pub system: String,
    pub tint: Cable,
    pub inputs: Vec<Port>,
    pub outputs: Vec<Port>,
    pub phase: Phase,
    pub summary: String,
    pub progress: Option<(usize, usize)>,
    pub body: Body,
    pub home: (f64, f64),
    pub width: f64,
    pub since: Option<i64>,
}

impl Node {
    pub fn passed(&self) -> bool {
        self.phase == Phase::Done && self.progress.is_none_or(|(done, total)| done == total)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WireState {
    Dormant,
    Live,
    Flowing,
}

impl WireState {
    pub fn class(self) -> &'static str {
        match self {
            Self::Dormant => "nf-cord",
            Self::Live => "nf-cord is-live",
            Self::Flowing => "nf-cord is-flowing",
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct Wire {
    pub from: NodeId,
    pub output: usize,
    pub to: NodeId,
    pub input: usize,
    pub cable: Cable,
    pub state: WireState,
}

#[derive(Clone, PartialEq, Default)]
pub struct Snapshot {
    pub detail: Option<ProjectDetail>,
    pub overview: Option<BuildOverview>,
    pub comfy: Option<ComfyOverview>,
    pub pinterest: Option<PinterestStatus>,
    pub hunt: Option<AssetHunt>,
    pub mixamo: Option<MixamoOverview>,
    pub open_design: Option<OpenDesignBoard>,
}

impl Snapshot {
    pub fn project_id(&self) -> Option<i64> {
        self.detail.as_ref().map(|detail| detail.project.id)
    }

    pub fn project_motions(&self) -> Vec<&MotionOrder> {
        match (self.project_id(), &self.mixamo) {
            (Some(project_id), Some(mixamo)) => mixamo.for_project(project_id),
            _ => Vec::new(),
        }
    }

    pub fn project_tasks(&self) -> Vec<&AssetTask> {
        let (Some(detail), Some(comfy)) = (&self.detail, &self.comfy) else {
            return Vec::new();
        };
        comfy
            .tasks
            .iter()
            .filter(|task| task.project_name.as_deref() == Some(detail.project.name.as_str()))
            .collect()
    }

    pub fn build_targets(&self, draft: &[BuildTarget]) -> Vec<BuildTarget> {
        let Some(overview) = &self.overview else {
            return if self.detail.is_none() { draft.to_vec() } else { Vec::new() };
        };
        BuildTarget::ALL
            .into_iter()
            .filter(|target| {
                overview.planned.contains(target)
                    || overview
                        .builds
                        .iter()
                        .any(|build| build.target == *target && build.status != BuildStatus::Cancelled)
            })
            .collect()
    }
}

pub fn latest_build(overview: &BuildOverview, target: BuildTarget) -> Option<&Build> {
    overview
        .builds
        .iter()
        .filter(|build| build.target == target && build.status != BuildStatus::Cancelled)
        .max_by_key(|build| build.id)
}

pub struct Draft<'a> {
    pub prompt: &'a str,
    pub engine: GameEngine,
    pub targets: &'a [BuildTarget],
}

#[derive(Clone, PartialEq, Debug)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub wires: Vec<Wire>,
}

impl Graph {
    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.iter().find(|node| node.id == id)
    }

    pub fn live(&self) -> Option<&Node> {
        self.nodes
            .iter()
            .filter(|node| node.phase.urgency() > 0)
            .fold(None, |best: Option<&Node>, node| match best {
                Some(kept) if kept.phase.urgency() >= node.phase.urgency() => Some(kept),
                _ => Some(node),
            })
    }

    pub fn patched_inputs(&self, id: NodeId, count: usize) -> Vec<bool> {
        (0..count)
            .map(|index| self.wires.iter().any(|wire| wire.to == id && wire.input == index))
            .collect()
    }

    pub fn patched_outputs(&self, id: NodeId, count: usize) -> Vec<bool> {
        (0..count)
            .map(|index| self.wires.iter().any(|wire| wire.from == id && wire.output == index))
            .collect()
    }
}

pub fn build_graph(snapshot: &Snapshot, draft: &Draft) -> Graph {
    let detail = snapshot.detail.as_ref();
    let planned = detail.is_some_and(|detail| {
        matches!(detail.project.status, ProjectStatus::Designing | ProjectStatus::Ready)
    });
    let targets = snapshot.build_targets(draft.targets);

    let mut nodes = vec![
        prompt_node(detail, draft),
        plan_node(detail),
        references_node(detail, snapshot.pinterest.as_ref(), planned),
        skills_node(NodeId::Foundation, detail, planned),
        skills_node(NodeId::Skills, detail, planned),
        screens_node(detail, planned),
        code_node(detail, snapshot.overview.as_ref(), draft.engine),
        assets_node(&snapshot.project_tasks(), snapshot.overview.as_ref()),
        finds_node(detail, snapshot.hunt.as_ref(), planned),
        animations_node(snapshot, draft.engine),
        open_design_node(detail, snapshot.open_design.as_ref(), planned),
        playtest_node(snapshot.overview.as_ref(), detail.map_or(draft.engine, |detail| detail.project.engine)),
    ];
    if targets.is_empty() {
        nodes.push(builds_placeholder(detail.is_some()));
    } else {
        nodes.extend(
            targets
                .iter()
                .enumerate()
                .map(|(index, target)| build_node(*target, index, snapshot.overview.as_ref())),
        );
    }

    let mut links = vec![
        (NodeId::Prompt, 0, NodeId::Plan, 0),
        (NodeId::Plan, 0, NodeId::References, 0),
        (NodeId::Plan, 0, NodeId::Foundation, 0),
        (NodeId::Foundation, 0, NodeId::Skills, 0),
        (NodeId::Plan, 1, NodeId::Screens, 0),
        (NodeId::References, 0, NodeId::Screens, 1),
        (NodeId::Foundation, 0, NodeId::Screens, 2),
        (NodeId::Screens, 0, NodeId::Code, 0),
        (NodeId::Skills, 0, NodeId::Code, 1),
        (NodeId::Code, 1, NodeId::Assets, 0),
        (NodeId::References, 0, NodeId::Assets, 1),
        (NodeId::Plan, 0, NodeId::Finds, 0),
        (NodeId::References, 0, NodeId::Finds, 1),
        (NodeId::Finds, 0, NodeId::Code, 2),
        (NodeId::Code, 1, NodeId::Animations, 0),
        (NodeId::Plan, 1, NodeId::OpenDesign, 0),
        (NodeId::References, 0, NodeId::OpenDesign, 1),
        (NodeId::Code, 0, NodeId::Playtest, 0),
        (NodeId::Screens, 0, NodeId::Playtest, 1),
    ];
    let tested = playtest_feeds_builds(snapshot.overview.as_ref(), detail.map_or(draft.engine, |detail| detail.project.engine));
    let sinks: Vec<NodeId> = if targets.is_empty() {
        vec![NodeId::Builds]
    } else {
        targets.iter().map(|target| NodeId::Build(*target)).collect()
    };
    for sink in sinks {
        links.push(if tested {
            (NodeId::Playtest, 0, sink, 0)
        } else {
            (NodeId::Code, 0, sink, 0)
        });
        links.push((NodeId::Assets, 0, sink, 1));
        links.push((NodeId::Animations, 0, sink, 2));
    }

    let wires = links
        .into_iter()
        .filter_map(|(from, output, to, input)| {
            let source = nodes.iter().find(|node| node.id == from)?;
            let target = nodes.iter().find(|node| node.id == to)?;
            let cable = source.outputs.get(output)?.cable;
            let state = if target.phase == Phase::Working {
                WireState::Flowing
            } else if source.phase == Phase::Done {
                WireState::Live
            } else {
                WireState::Dormant
            };
            Some(Wire {
                from,
                output,
                to,
                input,
                cable,
                state,
            })
        })
        .collect();

    Graph { nodes, wires }
}

fn shell(id: NodeId, title: &str, system: &str, tint: Cable, inputs: &[Port], outputs: &[Port]) -> Node {
    Node {
        id,
        title: title.to_owned(),
        system: system.to_owned(),
        tint,
        inputs: inputs.to_vec(),
        outputs: outputs.to_vec(),
        phase: Phase::Idle,
        summary: String::new(),
        progress: None,
        body: Body::Empty,
        home: home_of(id),
        width: NODE_WIDTH,
        since: None,
    }
}

fn home_of(id: NodeId) -> (f64, f64) {
    match id {
        NodeId::Prompt => (0.0, 170.0),
        NodeId::Plan => (COLUMN, 150.0),
        NodeId::References => (COLUMN * 2.0, 0.0),
        NodeId::Foundation => (COLUMN * 2.0, 330.0),
        NodeId::Screens => (COLUMN * 3.0, 0.0),
        NodeId::Skills => (COLUMN * 3.0, 380.0),
        NodeId::Code => (COLUMN * 4.0, 170.0),
        NodeId::Assets => (COLUMN * 5.0, 20.0),
        NodeId::Finds => (COLUMN * 3.0, 700.0),
        NodeId::Animations => (COLUMN * 5.0, 380.0),
        NodeId::OpenDesign => (COLUMN * 4.0, 560.0),
        NodeId::Playtest => (COLUMN * 6.0, 150.0),
        NodeId::Builds => (COLUMN * 7.0, 120.0),
        NodeId::Build(_) => (COLUMN * 7.0, 0.0),
    }
}

fn prompt_node(detail: Option<&ProjectDetail>, draft: &Draft) -> Node {
    let mut node = shell(NodeId::Prompt, "Промпт гри", "ти", Cable::Brief, &[], &[IDEA]);
    match detail {
        Some(detail) => {
            node.phase = Phase::Done;
            node.body = Body::Prompt {
                text: detail.project.prompt.clone(),
                chips: vec![detail.project.engine.label().to_owned()],
            };
        }
        None => {
            let typed = draft.prompt.trim();
            node.phase = Phase::Review;
            node.summary = if typed.is_empty() {
                "Опиши гру — фабрика запуститься звідси".to_owned()
            } else {
                "Готово до запуску".to_owned()
            };
            let mut chips = vec![draft.engine.label().to_owned()];
            chips.extend(draft.targets.iter().map(|target| target.label().to_owned()));
            node.body = Body::Prompt {
                text: typed.to_owned(),
                chips,
            };
        }
    }
    node
}

fn plan_node(detail: Option<&ProjectDetail>) -> Node {
    let mut node = shell(NodeId::Plan, "План гри", "Claude", Cable::Brief, &[IDEA], &[DOCUMENT, STYLE]);
    let Some(detail) = detail else {
        node.summary = "Чекає на промпт".to_owned();
        return node;
    };
    let project = &detail.project;
    match project.status {
        ProjectStatus::Queued => {
            node.phase = Phase::Queued;
            node.summary = "У черзі на планування".to_owned();
        }
        ProjectStatus::Planning => {
            node.phase = Phase::Working;
            node.summary = "Claude складає назву, стиль, дизайн-документ і екрани".to_owned();
            node.since = Some(project.created_at);
        }
        ProjectStatus::Failed => {
            node.phase = Phase::Failed;
            node.summary = project
                .error
                .as_ref()
                .map_or_else(|| "Планування не вдалося".to_owned(), explain);
        }
        ProjectStatus::Designing | ProjectStatus::Ready => {
            node.phase = Phase::Done;
            node.body = Body::Plan {
                name: project.name.clone(),
                pitch: project.pitch.clone(),
                palette: project.style.palette.clone(),
                chips: vec![plural(detail.screens.len() as u32, ["екран", "екрани", "екранів"])],
            };
        }
    }
    node
}

fn references_node(detail: Option<&ProjectDetail>, pinterest: Option<&PinterestStatus>, planned: bool) -> Node {
    let mut node = shell(NodeId::References, "Референси", "Pinterest", Cable::Visual, &[DOCUMENT], &[BOARD]);
    let Some(detail) = detail.filter(|_| planned) else {
        node.summary = "Підберуться за планом гри".to_owned();
        return node;
    };
    let project = &detail.project;
    let visual: Vec<_> = project
        .references
        .iter()
        .filter(|reference| !reference.topic.is_audio())
        .collect();
    let pinned = visual.iter().filter(|reference| has_pictures(reference)).count();
    let loading = pinterest.is_some_and(|status| status.is_loading(project.id));
    let images: Vec<String> = visual
        .iter()
        .filter_map(|reference| {
            reference
                .pins
                .first()
                .map(|pin| pin.image.clone())
                .or_else(|| reference.artworks.first().map(|artwork| artwork.image.clone()))
        })
        .take(VISIBLE_IMAGES)
        .collect();

    node.progress = (!visual.is_empty()).then_some((pinned, visual.len()));
    if project.references.is_empty() {
        node.summary = "Дошка порожня — підбери референси".to_owned();
        node.phase = Phase::Review;
    } else if loading {
        node.phase = Phase::Working;
        node.summary = "Шукає піни для кожної теми".to_owned();
    } else if pinned < visual.len() && pinterest.is_some_and(|status| status.signed_in == Some(false)) {
        node.phase = Phase::Review;
        node.summary = "Увійди в Pinterest, щоб підтягнути піни".to_owned();
    } else {
        node.phase = Phase::Done;
        node.summary = format!(
            "{} на дошці",
            plural(project.references.len() as u32, ["референс", "референси", "референсів"])
        );
    }
    node.body = if images.is_empty() {
        let rows: Vec<Row> = project
            .references
            .iter()
            .map(|reference| Row {
                phase: if has_pictures(reference) { Phase::Done } else { Phase::Idle },
                text: reference.title.clone(),
                tag: Some(reference.topic.label().to_owned()),
            })
            .collect();
        trimmed_rows(rows)
    } else {
        Body::Gallery { images }
    };
    node
}

fn has_pictures(reference: &Reference) -> bool {
    !reference.pins.is_empty() || !reference.artworks.is_empty()
}

pub fn stage_outputs(detail: &ProjectDetail, id: NodeId) -> Vec<&SkillOutput> {
    detail
        .outputs
        .iter()
        .filter(|output| match id {
            NodeId::Foundation => output.stage <= DESIGN_STAGE,
            _ => output.stage > DESIGN_STAGE,
        })
        .collect()
}

fn skills_node(id: NodeId, detail: Option<&ProjectDetail>, planned: bool) -> Node {
    let (title, outputs_port) = match id {
        NodeId::Foundation => ("Навички етапу 01", TEXTS),
        _ => ("Навички етапів 02–05", TEXTS),
    };
    let inputs: &[Port] = match id {
        NodeId::Foundation => &[DOCUMENT],
        _ => &[TEXTS],
    };
    let mut node = shell(id, title, "SKILLS", Cable::Brief, inputs, &[outputs_port]);
    let Some(detail) = detail.filter(|_| planned) else {
        node.summary = "Запустяться після плану".to_owned();
        return node;
    };
    let outputs = stage_outputs(detail, id);
    if outputs.is_empty() {
        node.summary = "Немає увімкнених навичок — додай їх у SKILLS".to_owned();
        return node;
    }
    let rows: Vec<Row> = outputs
        .iter()
        .map(|output| Row {
            phase: output_phase(output.status),
            text: output.title.clone(),
            tag: Some(format!("{:02}", output.stage)),
        })
        .collect();
    node.phase = strongest(&rows);
    node.progress = Some((count(&rows, Phase::Done), rows.len()));
    node.summary = group_summary(
        node.phase,
        &rows,
        "Усі документи готові",
        |title| format!("«{title}» пише документ"),
        |title| format!("«{title}» не відпрацювала"),
        ["документ", "документи", "документів"],
    );
    node.body = trimmed_rows(rows);
    node
}

fn screens_node(detail: Option<&ProjectDetail>, planned: bool) -> Node {
    let mut node = shell(
        NodeId::Screens,
        "Екрани гри",
        "Claude Design",
        Cable::Visual,
        &[STYLE, BOARD, TEXTS],
        &[SCREENS],
    );
    node.width = WIDE_NODE_WIDTH;
    let Some(detail) = detail.filter(|_| planned) else {
        node.summary = "Намалюються за стилем і референсами".to_owned();
        return node;
    };
    if detail.screens.is_empty() {
        node.summary = "У плані немає екранів".to_owned();
        return node;
    }
    let rows: Vec<Row> = detail
        .screens
        .iter()
        .map(|screen| Row {
            phase: screen_phase(screen.status),
            text: screen.title.clone(),
            tag: None,
        })
        .collect();
    node.phase = strongest(&rows);
    node.progress = Some((count(&rows, Phase::Done), rows.len()));
    node.summary = match node.phase {
        Phase::Review => format!(
            "Апруву чекають {}",
            plural(count(&rows, Phase::Review) as u32, ["екран", "екрани", "екранів"])
        ),
        _ => group_summary(
            node.phase,
            &rows,
            "Усі екрани апрувнуто",
            |title| format!("Малює «{title}»"),
            |title| format!("Екран «{title}» не згенеровано"),
            ["екран", "екрани", "екранів"],
        ),
    };
    node.body = trimmed_rows(rows);
    node
}

fn code_node(detail: Option<&ProjectDetail>, overview: Option<&BuildOverview>, draft_engine: GameEngine) -> Node {
    let engine = overview
        .map(|overview| overview.engine)
        .or_else(|| detail.map(|detail| detail.project.engine))
        .unwrap_or(draft_engine);
    let mut node = shell(
        NodeId::Code,
        "Код гри",
        engine.label(),
        Cable::Code,
        &[SCREENS, TEXTS, FOUND],
        &[SOURCE, ORDERS],
    );
    let source = overview.and_then(|overview| overview.source.as_ref());
    match source {
        None if detail.is_some_and(ready_for_code) => {
            node.phase = Phase::Queued;
            node.summary = "Екрани й навички готові — агент стартує сам".to_owned();
        }
        None => {
            node.summary = "Агент почне, щойно екрани й навички будуть готові".to_owned();
        }
        Some(source) => {
            let lines: Vec<String> = source
                .activity
                .iter()
                .rev()
                .take(VISIBLE_LOG_LINES)
                .rev()
                .cloned()
                .collect();
            match source.status {
                SourceStatus::Generating => {
                    node.phase = Phase::Working;
                    node.summary = "Claude-агент пише й перевіряє код".to_owned();
                }
                SourceStatus::Ready if source.valid => {
                    node.phase = Phase::Done;
                    node.summary = "Код гри готовий".to_owned();
                }
                SourceStatus::Ready => {
                    node.phase = Phase::Failed;
                    node.summary = "Папка не схожа на проєкт рушія".to_owned();
                }
                SourceStatus::Failed => {
                    node.phase = Phase::Failed;
                    node.summary = source
                        .error
                        .clone()
                        .unwrap_or_else(|| "Генерація коду не вдалася".to_owned());
                }
            }
            if !lines.is_empty() {
                node.body = Body::Log { lines };
            }
        }
    }
    node
}

fn ready_for_code(detail: &ProjectDetail) -> bool {
    matches!(detail.project.status, ProjectStatus::Designing | ProjectStatus::Ready)
        && !detail.screens.is_empty()
        && detail
            .screens
            .iter()
            .all(|screen| matches!(screen.status, ScreenStatus::Draft | ScreenStatus::Approved))
        && detail.outputs.iter().all(|output| output.status == OutputStatus::Done)
}

fn assets_node(tasks: &[&AssetTask], overview: Option<&BuildOverview>) -> Node {
    let mut node = shell(NodeId::Assets, "Ассети", "ComfyUI", Cable::Visual, &[ORDERS, BOARD], &[FILES]);
    if tasks.is_empty() {
        let coding = overview
            .and_then(|overview| overview.source.as_ref())
            .is_some_and(|source| source.status == SourceStatus::Generating);
        node.summary = if coding {
            "Агент ще не замовив жодного ассета".to_owned()
        } else {
            "Агент замовить арт, звук і 3D, коли писатиме код".to_owned()
        };
        return node;
    }
    let rows: Vec<Row> = tasks
        .iter()
        .map(|task| Row {
            phase: task_phase(task.status),
            text: task.title.clone(),
            tag: Some(task.kind.label().to_owned()),
        })
        .collect();
    let runs = tasks.iter().flat_map(|task| task.runs.iter());
    let (finished, total) = runs.fold((0, 0), |(done, all), run| {
        (done + usize::from(run.status == RunStatus::Done), all + 1)
    });
    node.phase = strongest(&rows);
    node.progress = (total > 0).then_some((finished, total));
    node.since = tasks
        .iter()
        .flat_map(|task| task.runs.iter())
        .filter(|run| run.status == RunStatus::Running)
        .filter_map(|run| run.started_at)
        .min();
    node.summary = group_summary(
        node.phase,
        &rows,
        "Усі ассети згенеровано",
        |title| format!("Генерує «{title}»"),
        |title| format!("«{title}» не вдалося"),
        ["задача", "задачі", "задач"],
    );
    let images: Vec<String> = tasks
        .iter()
        .flat_map(|task| task.runs.iter())
        .flat_map(|run| run.files.iter())
        .filter(|file| file.medium == Medium::Image)
        .map(|file| file.url.clone())
        .take(VISIBLE_IMAGES)
        .collect();
    node.body = if images.is_empty() {
        trimmed_rows(rows)
    } else {
        Body::Gallery { images }
    };
    node
}

fn finds_node(detail: Option<&ProjectDetail>, hunt: Option<&AssetHunt>, planned: bool) -> Node {
    let mut node = shell(
        NodeId::Finds,
        HUNT_TITLE,
        HUNT_NOTE,
        Cable::Visual,
        &[DOCUMENT, BOARD],
        &[FOUND],
    );
    if detail.filter(|_| planned).is_none() {
        node.summary = "Почне шукати, щойно буде план гри".to_owned();
        return node;
    }
    let Some(hunt) = hunt else {
        node.summary = "Ще не шукали — відкрий вузол і натисни «Знайти ассети»".to_owned();
        return node;
    };
    match hunt.status {
        HuntStatus::Queued => {
            node.phase = Phase::Queued;
            node.summary = "У черзі на пошук".to_owned();
        }
        HuntStatus::Searching => {
            node.phase = Phase::Working;
            node.summary = searching_note();
            node.since = hunt.started_at;
        }
        HuntStatus::Done => {
            node.phase = Phase::Done;
            node.summary = found_summary(hunt);
        }
        HuntStatus::Failed => {
            node.phase = Phase::Failed;
            node.summary = hunt
                .error
                .as_ref()
                .map_or_else(|| "Пошук не вдався".to_owned(), explain);
        }
    }
    let images: Vec<String> = hunt
        .finds
        .iter()
        .filter_map(|find| find.image.clone())
        .take(VISIBLE_IMAGES)
        .collect();
    node.body = if images.is_empty() {
        trimmed_rows(
            hunt.finds
                .iter()
                .map(|find| Row {
                    phase: if find.cost.downloadable() { Phase::Done } else { Phase::Idle },
                    text: find.title.clone(),
                    tag: Some(find.kind.label().to_owned()),
                })
                .collect(),
        )
    } else {
        Body::Gallery { images }
    };
    node
}

fn open_design_node(detail: Option<&ProjectDetail>, board: Option<&OpenDesignBoard>, planned: bool) -> Node {
    let mut node = shell(
        NodeId::OpenDesign,
        "Варіації дизайну",
        "Open Design",
        Cable::Visual,
        &[STYLE, BOARD],
        &[VARIANTS],
    );
    let Some(detail) = detail.filter(|_| planned) else {
        node.summary = "Почне разом із Claude Design".to_owned();
        return node;
    };
    let Some(board) = board else {
        node.summary = "Перевіряємо Open Design…".to_owned();
        return node;
    };
    if !board.installed {
        node.phase = Phase::Review;
        node.summary = "Open Design не встановлено".to_owned();
        return node;
    }
    let screens: Vec<u32> = detail.screens.iter().map(|screen| screen.position).collect();
    let covered = screens
        .iter()
        .filter(|position| !board.for_screen(**position).is_empty())
        .count();
    node.progress = (!screens.is_empty() && board.rounds > 0).then_some((covered, screens.len()));
    match board.status {
        DesignRunStatus::WaitingReferences => {
            node.phase = Phase::Queued;
            node.summary = "Чекає на референси Pinterest і ArtStation".to_owned();
            node.since = board.started_at;
        }
        DesignRunStatus::Starting | DesignRunStatus::Running => {
            node.phase = Phase::Working;
            node.summary = format!(
                "Генерує · {}",
                plural(board.variants.len() as u32, VARIANT_WORDS)
            );
            node.since = board.started_at;
        }
        DesignRunStatus::Done => {
            node.phase = Phase::Done;
            node.summary = variants_summary(board);
        }
        DesignRunStatus::Failed => {
            node.phase = Phase::Failed;
            node.summary = board
                .error
                .clone()
                .unwrap_or_else(|| status_label(board.status).to_owned());
        }
        DesignRunStatus::Idle => {
            node.summary = "Ще не запускали — відкрий вузол".to_owned();
        }
    }
    node.body = trimmed_rows(
        board
            .variants
            .iter()
            .map(|variant| Row {
                phase: Phase::Done,
                text: variant.title.clone(),
                tag: Some(match variant.screen {
                    Some(position) => format!("{position:02} · {}", variant.source.short_label()),
                    None => variant.source.short_label().to_owned(),
                }),
            })
            .collect(),
    );
    node
}

fn animations_node(snapshot: &Snapshot, draft_engine: GameEngine) -> Node {
    let mut node = shell(NodeId::Animations, "Анімації", "Mixamo", Cable::Visual, &[ORDERS], &[MOTIONS]);
    let detail = snapshot.detail.as_ref();
    let engine = detail.map_or(draft_engine, |detail| detail.project.engine);
    let signed_in = snapshot.mixamo.as_ref().and_then(|mixamo| mixamo.status.signed_in);
    let orders = snapshot.project_motions();
    if orders.is_empty() {
        let coding = snapshot
            .overview
            .as_ref()
            .and_then(|overview| overview.source.as_ref())
            .is_some_and(|source| source.status == SourceStatus::Generating);
        node.summary = if !engine_takes_motions(engine) {
            format!("{} — 2D-рушій, анімації скелета не потрібні", engine.label())
        } else if signed_in == Some(false) {
            "Увійди в Mixamo — і агент братиме анімації звідти".to_owned()
        } else if detail.is_none() {
            "Анімації персонажів прийдуть з Mixamo".to_owned()
        } else if coding {
            "Агент ще не замовив анімацій".to_owned()
        } else {
            "Агент замовить анімації з Mixamo, коли писатиме код".to_owned()
        };
        return node;
    }
    let rows: Vec<Row> = orders
        .iter()
        .map(|order| Row {
            phase: motion_phase(order.status),
            text: order.name.clone(),
            tag: order.clip.as_ref().map(|clip| clip.name.clone()).filter(|name| !name.is_empty()),
        })
        .collect();
    let waiting = orders.iter().filter(|order| order.status == MotionStatus::Queued).count();
    node.progress = Some((count(&rows, Phase::Done), rows.len()));
    if signed_in == Some(false) && waiting > 0 {
        node.phase = Phase::Review;
        node.summary = format!(
            "Увійди в Mixamo — {} чекають у черзі",
            plural(waiting as u32, ["анімація", "анімації", "анімацій"])
        );
    } else {
        node.phase = strongest(&rows);
        node.summary = group_summary(
            node.phase,
            &rows,
            "Усі анімації запечено",
            |title| format!("Запікає «{title}»"),
            |title| format!("«{title}» не вдалося"),
            ["анімація", "анімації", "анімацій"],
        );
    }
    let images: Vec<String> = orders
        .iter()
        .filter_map(|order| order.clip.as_ref())
        .map(|clip| clip.preview().to_owned())
        .filter(|preview| !preview.is_empty())
        .take(VISIBLE_IMAGES)
        .collect();
    node.body = if images.is_empty() {
        trimmed_rows(rows)
    } else {
        Body::Gallery { images }
    };
    node
}

pub fn motion_phase(status: MotionStatus) -> Phase {
    match status {
        MotionStatus::Queued => Phase::Queued,
        MotionStatus::Searching | MotionStatus::Exporting | MotionStatus::Downloading => Phase::Working,
        MotionStatus::Done => Phase::Done,
        MotionStatus::Failed => Phase::Failed,
    }
}

fn playtest_feeds_builds(overview: Option<&BuildOverview>, engine: GameEngine) -> bool {
    overview.map_or(engine == GameEngine::Godot, |overview| overview.playtest.blocked.is_none())
}

fn playtest_node(overview: Option<&BuildOverview>, engine: GameEngine) -> Node {
    let mut node = shell(
        NodeId::Playtest,
        "Плейтест",
        "бот · Claude",
        Cable::Code,
        &[SOURCE, SCREENS],
        &[TESTED],
    );
    node.width = WIDE_NODE_WIDTH;
    let Some(overview) = overview else {
        node.summary = if engine == GameEngine::Godot {
            "Бот зіграє в гру, щойно буде код".to_owned()
        } else {
            "Поки грає лише в ігри на Godot".to_owned()
        };
        return node;
    };
    let playtest = &overview.playtest;
    let Some(run) = &playtest.run else {
        node.summary = match &playtest.blocked {
            Some(reason) => reason.clone(),
            None if overview.can_build() => "Готовий зіграти — відкрий вузол".to_owned(),
            None => "Зіграє сам, щойно код гри буде готовий".to_owned(),
        };
        return node;
    };
    node.since = run.status.is_active().then_some(run.started_at);
    match run.status {
        PlaytestStatus::Capturing => {
            node.phase = Phase::Working;
            node.summary = if !run.activity.is_empty() {
                "Бот грає фінальний прогін".to_owned()
            } else {
                "Бот грає в гру й знімає кадри".to_owned()
            };
        }
        PlaytestStatus::Reviewing => {
            node.phase = Phase::Working;
            node.summary = "Claude дивиться кадри й виправляє гру".to_owned();
        }
        PlaytestStatus::Failed => {
            node.phase = Phase::Failed;
            node.summary = run.error.clone().unwrap_or_else(|| "Плейтест не вдався".to_owned());
        }
        PlaytestStatus::Done => match &run.report {
            None => {
                node.phase = Phase::Done;
                node.summary = "Плейтест завершено".to_owned();
            }
            Some(report) => {
                let open = report.open();
                node.progress = (!report.issues.is_empty()).then_some((report.fixed(), report.issues.len()));
                (node.phase, node.summary) = match report.verdict {
                    PlaytestVerdict::Passed => (Phase::Done, "Гра пройшла плейтест".to_owned()),
                    PlaytestVerdict::Fixed if open == 0 => (
                        Phase::Done,
                        format!("Виправлено {}", plural(report.fixed() as u32, ["проблему", "проблеми", "проблем"])),
                    ),
                    PlaytestVerdict::Fixed | PlaytestVerdict::NeedsWork if open > 0 => (
                        Phase::Review,
                        format!("Лишилось {} — глянь звіт", plural(open as u32, ["проблема", "проблеми", "проблем"])),
                    ),
                    PlaytestVerdict::Fixed | PlaytestVerdict::NeedsWork => {
                        (Phase::Review, "Гра ще потребує доробки — глянь звіт".to_owned())
                    }
                };
            }
        },
    }
    let frames: Vec<String> = run
        .shots
        .iter()
        .rev()
        .take(VISIBLE_FRAMES)
        .rev()
        .map(|shot| shot.thumb.clone())
        .collect();
    node.body = if !frames.is_empty() {
        Body::Frames { images: frames }
    } else if !run.activity.is_empty() {
        Body::Log {
            lines: run.activity.iter().rev().take(VISIBLE_LOG_LINES).rev().cloned().collect(),
        }
    } else {
        Body::Empty
    };
    node
}

fn builds_placeholder(has_project: bool) -> Node {
    let mut node = shell(
        NodeId::Builds,
        "Білди",
        "платформи",
        Cable::Package,
        &[SOURCE, FILES, MOTIONS],
        &[PACKAGE],
    );
    node.summary = if has_project {
        "Платформи не обрано — вибери їх тут".to_owned()
    } else {
        "Обери платформи у вузлі «Промпт гри»".to_owned()
    };
    node
}

fn build_node(target: BuildTarget, index: usize, overview: Option<&BuildOverview>) -> Node {
    let mut node = shell(
        NodeId::Build(target),
        &format!("Білд {}", target.label()),
        target.package(),
        Cable::Package,
        &[SOURCE, FILES, MOTIONS],
        &[PACKAGE],
    );
    let (x, y) = home_of(node.id);
    node.home = (x, y + index as f64 * BUILD_SPACING);
    let Some(overview) = overview else {
        node.summary = "Збереться після коду гри".to_owned();
        return node;
    };
    match latest_build(overview, target) {
        None => {
            node.summary = match overview.readiness(target) {
                Readiness::Ready(_) if overview.can_build() => "Можна збирати".to_owned(),
                Readiness::Ready(_) => "Збереться сам після коду гри".to_owned(),
                Readiness::Missing(missing) => format!("Бракує: {missing}"),
                Readiness::NeedsHost(note) => note,
                Readiness::Unsupported => "Рушій не збирає під цю платформу".to_owned(),
            };
        }
        Some(build) => match build.status {
            BuildStatus::Queued => {
                node.phase = Phase::Queued;
                node.summary = "У черзі збірки".to_owned();
            }
            BuildStatus::Running => {
                node.phase = Phase::Working;
                node.since = build.started_at;
                node.summary = last_line(&build.log_tail).unwrap_or_else(|| "Збирається".to_owned());
            }
            BuildStatus::Done => {
                node.phase = Phase::Done;
                node.summary = build
                    .size_bytes
                    .map_or_else(|| "Білд готовий".to_owned(), |size| format!("Білд готовий, {}", format_size(size)));
            }
            BuildStatus::Failed => {
                node.phase = Phase::Failed;
                node.summary = build.error.clone().unwrap_or_else(|| "Білд упав".to_owned());
            }
            BuildStatus::Cancelled => {}
        },
    }
    node
}

pub fn last_line(text: &str) -> Option<String> {
    text.lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_owned)
}

pub fn output_phase(status: OutputStatus) -> Phase {
    match status {
        OutputStatus::Queued => Phase::Queued,
        OutputStatus::Generating => Phase::Working,
        OutputStatus::Done => Phase::Done,
        OutputStatus::Failed => Phase::Failed,
    }
}

pub fn screen_phase(status: ScreenStatus) -> Phase {
    match status {
        ScreenStatus::Queued => Phase::Queued,
        ScreenStatus::Generating => Phase::Working,
        ScreenStatus::Draft => Phase::Review,
        ScreenStatus::Approved => Phase::Done,
        ScreenStatus::Failed => Phase::Failed,
    }
}

pub fn task_phase(status: TaskStatus) -> Phase {
    match status {
        TaskStatus::Writing | TaskStatus::Running => Phase::Working,
        TaskStatus::Queued => Phase::Queued,
        TaskStatus::Done => Phase::Done,
        TaskStatus::Failed => Phase::Failed,
    }
}

pub fn run_phase(status: RunStatus) -> Phase {
    match status {
        RunStatus::Queued => Phase::Queued,
        RunStatus::Running => Phase::Working,
        RunStatus::Done => Phase::Done,
        RunStatus::Failed => Phase::Failed,
        RunStatus::Skipped => Phase::Idle,
    }
}

pub fn build_phase(status: BuildStatus) -> Phase {
    match status {
        BuildStatus::Queued => Phase::Queued,
        BuildStatus::Running => Phase::Working,
        BuildStatus::Done => Phase::Done,
        BuildStatus::Failed => Phase::Failed,
        BuildStatus::Cancelled => Phase::Idle,
    }
}

fn strongest(rows: &[Row]) -> Phase {
    rows.iter().map(|row| row.phase).max().unwrap_or(Phase::Idle)
}

fn count(rows: &[Row], phase: Phase) -> usize {
    rows.iter().filter(|row| row.phase == phase).count()
}

fn group_summary(
    phase: Phase,
    rows: &[Row],
    finished: &str,
    working: impl Fn(&str) -> String,
    failed: impl Fn(&str) -> String,
    units: [&str; 3],
) -> String {
    let lead = rows
        .iter()
        .find(|row| row.phase == phase)
        .map_or("", |row| row.text.as_str());
    match phase {
        Phase::Idle => String::new(),
        Phase::Done => finished.to_owned(),
        Phase::Working => working(lead),
        Phase::Failed => failed(lead),
        Phase::Queued => format!("{} у черзі", plural(count(rows, Phase::Queued) as u32, units)),
        Phase::Review => format!("{} чекають на тебе", plural(count(rows, Phase::Review) as u32, units)),
    }
}

fn trimmed_rows(rows: Vec<Row>) -> Body {
    let hidden = rows.len().saturating_sub(VISIBLE_ROWS);
    Body::Rows {
        rows: rows.into_iter().take(VISIBLE_ROWS).collect(),
        hidden,
    }
}
