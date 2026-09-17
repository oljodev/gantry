//! First-party connector: Media generation (`docs/plan/03-connector-system.md` §5, 02 §4b).
//!
//! The other direction from the media *routing*. Routing is for a turn whose chosen model draws
//! or speaks: you pick the image model in the picker and talk to it. This is for a chat model
//! that wants a picture as one step of its own work — it stays the model you are talking to, and
//! calls a drawing model the way it calls any other tool.
//!
//! **One tool, and at most one generation per call.** There is no `n`, no batch and no loop:
//! one call is one request to one model, and a model that wants three pictures calls three
//! times. That is the shape of the thing being spent — each generation is money on the user's
//! own account — and a tool that could spend it a variable number of times per call would make
//! every permission card a guess.
//!
//! **What it hands back is two things.** The tool *result* is a sentence and some numbers for
//! the model to read; the picture itself goes to the answer through `ToolOutcome::media` (03
//! §4), which puts it in the reply where the call happened rather than leaving it buried in an
//! activity row. Nothing here writes to disk or to the blob store: the turn loop parks the
//! bytes exactly as it does for a media model's own answer.

#![forbid(unsafe_code)]

mod models;
mod settings;

use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use futures_util::StreamExt;
use gantry_connectors::{
    Connector, ConnectorDescriptor, ConnectorError, ToolCallRequest, ToolEventSink, ToolOutcome,
};
use gantry_core::{ContentPart, InstanceId, MediaOptions, Mode, RiskTier, Settings, ToolDef};
use gantry_providers::{ChatRequest, ProviderRegistry};
use gantry_store::Store;
use tokio_util::sync::CancellationToken;

pub use models::{
    Candidate, Kind, MAX_AGE_DAYS, Picked, check, choose, describe_kind, detail, kind_name, list,
    listing, matches, model_choice, named_by, parse_kind, pick, rank, row,
};
pub use settings::{
    AUTOMATIC, DEFAULT_MODEL_RULE, Preferences, Rule, fields as settings_form, key_for,
};

/// The settings form for an installed instance (03 §11 step 2), with the model menus filled in
/// from the models this machine can reach right now. `desktop/app/src/native.rs` asks for it
/// when the connector's settings are opened.
#[must_use]
pub fn settings_fields(
    providers: &Arc<ProviderRegistry>,
    store: &Arc<Store>,
) -> Vec<gantry_core::UserConfigField> {
    settings_form(&models::list(providers, store), gantry_core::now_ms())
}

/// The connector manifest, embedded at build time (03 §3).
pub const MANIFEST: &str = include_str!("../manifest.json");

pub const ID: &str = "media";

/// The longest prompt a generation endpoint is asked to read. These models take one string, and
/// a prompt past this length is a document rather than a description of a picture.
pub const MAX_PROMPT_CHARS: usize = 8_000;

/// How many models `list_models` reports when the call does not say. Enough to see the shape of
/// what is available without pasting a provider's whole catalogue into the conversation.
pub const DEFAULT_LISTED: usize = 40;

/// The most it will report however large a `limit` asks for.
pub const MAX_LISTED: usize = 200;

pub struct Media {
    descriptor: ConnectorDescriptor,
    providers: Arc<ProviderRegistry>,
    store: Arc<Store>,
    /// Read for `model_options`: the voice, shape and length the user already chose for a model
    /// in the picker (11 §1). A tool that ignored them would answer in a different voice from
    /// the one the same model uses when you talk to it directly.
    settings: Arc<RwLock<Settings>>,
}

impl Media {
    #[must_use]
    pub fn new(
        namespace: String,
        instance_id: InstanceId,
        providers: Arc<ProviderRegistry>,
        store: Arc<Store>,
        settings: Arc<RwLock<Settings>>,
    ) -> Self {
        Self {
            descriptor: ConnectorDescriptor {
                id: namespace,
                name: "Media generation".to_owned(),
                instance_id: Some(instance_id),
                first_party: true,
            },
            providers,
            store,
            settings,
        }
    }

    fn generate_args(&self, args: &serde_json::Value, mode: Mode) -> Result<Request, String> {
        let prompt = args
            .get("prompt")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_owned();
        if prompt.is_empty() {
            return Err("`prompt` is required: say what to make.".to_owned());
        }
        if prompt.chars().count() > MAX_PROMPT_CHARS {
            return Err(format!(
                "That prompt is {} characters; these models take at most {MAX_PROMPT_CHARS}.",
                prompt.chars().count()
            ));
        }
        let wanted = args
            .get("kind")
            .and_then(serde_json::Value::as_str)
            .map(models::parse_kind)
            .transpose()?;
        let named = args
            .get("model")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|m| !m.is_empty());

        let available = models::list(&self.providers, &self.store);
        let models::Picked { candidate, note } = models::pick(
            &available,
            gantry_core::now_ms(),
            &self.preferences(),
            mode,
            named,
            wanted,
        )
        .map_err(|e| self.with_the_list(e))?;
        let chosen = candidate;

        // What the user picked for this model in the dialog, then whatever the call names on
        // top of it. Nothing is invented: a field nobody chose is a field the request omits,
        // and a model's own default beats a guess (02 §4b).
        let mut options = self
            .settings
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .chat
            .model_options
            .get(&chosen.key())
            .cloned()
            .unwrap_or_default();
        apply(&mut options, args);
        models::check(&chosen, &options)?;

        Ok(Request {
            prompt,
            chosen,
            options,
            note,
        })
    }

    /// What the user chose in this connector's settings (03 §11 step 2).
    ///
    /// Read per call rather than held from when the connector was built: the settings page
    /// writes the answers and the turn loop keeps the same connector, so a value cached at
    /// build time would be the one the user had before they changed it.
    fn preferences(&self) -> settings::Preferences {
        let Some(id) = self.descriptor.instance_id else {
            return settings::Preferences::default();
        };
        let values = self
            .store
            .read(move |c| gantry_store::repos::connectors::user_config(c, id))
            .unwrap_or_default();
        settings::Preferences::read(&values)
    }

    /// A refusal about *which model* ends with the way to stop guessing. The name is the
    /// namespaced one, because that is the only name the model can actually call — and a model
    /// that has just been told a name does not exist will otherwise try another one it
    /// remembers rather than the list that is one call away.
    fn with_the_list(&self, message: String) -> String {
        format!(
            "{message} Call `{}__list_models` to see every model this machine can use.",
            self.descriptor.id
        )
    }

    /// The catalogue, as the model asked to see it.
    ///
    /// Nothing is requested from anybody: this reads the cached model list the provider registry
    /// already keeps (02 §2), so it costs nothing and cannot fail on the network. It exists
    /// because the alternative to a list is a memory — and a model id a chat model remembers
    /// from its training is exactly the kind of thing that has been renamed since.
    fn list_models(&self, args: &serde_json::Value) -> ToolOutcome {
        let wanted = match args
            .get("kind")
            .and_then(serde_json::Value::as_str)
            .map(models::parse_kind)
            .transpose()
        {
            Ok(kind) => kind,
            Err(message) => return ToolOutcome::error(message),
        };
        let search = args
            .get("search")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_owned();
        let limit = usize::try_from(
            args.get("limit")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(DEFAULT_LISTED as u64),
        )
        .unwrap_or(DEFAULT_LISTED)
        .clamp(1, MAX_LISTED);

        let available = models::list(&self.providers, &self.store);
        if available.is_empty() {
            return ToolOutcome::error(
                "There are no media models on this machine: the user needs a provider key, and \
                 the model list refreshed in Settings \u{2192} Providers.",
            );
        }
        let (summary, structured) =
            models::listing(&available, gantry_core::now_ms(), wanted, &search, limit);
        ToolOutcome::Complete {
            content: vec![
                gantry_core::ResultPart::Text { text: summary },
                gantry_core::ResultPart::Json {
                    json: structured.clone(),
                },
            ],
            structured: Some(structured),
            is_error: false,
            media: Vec::new(),
        }
    }

    async fn generate(
        &self,
        args: &serde_json::Value,
        req: &ToolCallRequest,
        sink: &Arc<dyn ToolEventSink>,
    ) -> ToolOutcome {
        let Request {
            prompt,
            chosen,
            options,
            note,
        } = match self.generate_args(args, req.scope.mode) {
            Ok(parsed) => parsed,
            Err(message) => return ToolOutcome::error(message),
        };
        let Some(provider) = self.providers.get(&chosen.provider) else {
            return ToolOutcome::error(format!(
                "The provider {} is not configured any more.",
                chosen.provider
            ));
        };
        if !provider.has_key() {
            return ToolOutcome::error(format!(
                "There is no API key for {}, so {} cannot be asked to make anything. \
                 The user adds one in Settings → Providers.",
                chosen.provider,
                chosen.key()
            ));
        }

        let mut request = ChatRequest::new(
            chosen.model.clone(),
            String::new(),
            vec![gantry_core::Message::user_text(prompt)],
        );
        request.media = options;
        // One generation per call, so one attempt: a retry here would be a second charge on
        // somebody's account for a request they asked for once.
        request.retries = 1;

        let stream = match provider.stream(request).await {
            Ok(stream) => stream,
            Err(err) => return ToolOutcome::error(format!("{} refused: {err}", chosen.key())),
        };
        let mut stream = stream;
        let mut parts: Vec<ContentPart> = Vec::new();
        let mut usage: Option<gantry_core::Usage> = None;
        while let Some(event) = stream.next().await {
            match event {
                Ok(gantry_providers::StreamEvent::ProviderBlock { part, .. })
                    if is_media(&part) =>
                {
                    parts.push(part);
                }
                // A clip takes minutes and the route says so every ten seconds (02 §4b). The
                // row shows it rather than the model: a progress line is for the person
                // watching, and a model reading "still rendering" four times learns nothing.
                Ok(gantry_providers::StreamEvent::Notice { detail, .. }) => {
                    sink.progress(&req.call_id, None, Some(detail));
                }
                Ok(gantry_providers::StreamEvent::Usage(u)) => usage = Some(u),
                Ok(_) => {}
                Err(err) => return ToolOutcome::error(format!("{} failed: {err}", chosen.key())),
            }
        }
        if parts.is_empty() {
            return ToolOutcome::error(format!(
                "{} answered without a {}.",
                chosen.key(),
                describe_kind(chosen.kind)
            ));
        }
        answer(&chosen, &parts, usage.as_ref(), note.as_deref())
    }
}

struct Request {
    prompt: String,
    chosen: Candidate,
    options: MediaOptions,
    /// Something the model should say in its own words, because the call did not do quite what
    /// the settings said it would.
    note: Option<String>,
}

fn is_media(part: &ContentPart) -> bool {
    matches!(
        part,
        ContentPart::Image { .. } | ContentPart::Audio { .. } | ContentPart::Video { .. }
    )
}

/// Options the call named, over whatever the user had already chosen for the model.
fn apply(options: &mut MediaOptions, args: &serde_json::Value) {
    let text = |key: &str| {
        args.get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_owned)
    };
    if let Some(v) = text("aspect_ratio") {
        options.aspect_ratio = Some(v);
    }
    if let Some(v) = text("resolution") {
        options.resolution = Some(v);
    }
    if let Some(v) = text("quality") {
        options.quality = Some(v);
    }
    if let Some(v) = text("voice") {
        options.voice = Some(v);
    }
    if let Some(v) = args
        .get("duration_seconds")
        .and_then(serde_json::Value::as_u64)
    {
        options.duration_seconds = u32::try_from(v).ok();
    }
}

/// The result the model reads, and the parts the reader sees.
///
/// The bytes are deliberately *not* in the result: a picture in a tool result is stored twice —
/// once in the call row and once in the transcript — and read whole out of SQLite every time the
/// chat is opened (02 §4b). What the model gets is what it needs to talk about the thing it
/// just made: which model made it, what kind of file it is, how big, and what it cost.
fn answer(
    chosen: &Candidate,
    parts: &[ContentPart],
    usage: Option<&gantry_core::Usage>,
    note: Option<&str>,
) -> ToolOutcome {
    let mime = parts.first().and_then(mime_of).unwrap_or_default();
    let bytes: usize = parts.iter().filter_map(size_of).sum();
    let cost = usage.and_then(|u| u.cost_usd);
    let mut summary = format!(
        "Made {} with {}. It is in the reply already — do not describe it as a link or a file \
         path, and do not try to read it back.",
        describe_kind(chosen.kind),
        chosen.key()
    );
    if let Some(cost) = cost {
        summary.push_str(&format!(" It cost ${cost:.4}."));
    }
    if let Some(note) = note {
        summary.push(' ');
        summary.push_str(note);
    }
    let structured = serde_json::json!({
        "model": chosen.key(),
        "kind": models::kind_name(chosen.kind),
        "mime": mime,
        "bytes": bytes,
        "cost_usd": cost,
        "count": parts.len(),
    });
    ToolOutcome::Complete {
        content: vec![
            gantry_core::ResultPart::Text { text: summary },
            gantry_core::ResultPart::Json {
                json: structured.clone(),
            },
        ],
        structured: Some(structured),
        is_error: false,
        media: parts.to_vec(),
    }
}

fn mime_of(part: &ContentPart) -> Option<String> {
    match part {
        ContentPart::Image { mime, .. }
        | ContentPart::Audio { mime, .. }
        | ContentPart::Video { mime, .. } => Some(mime.clone()),
        _ => None,
    }
}

/// The size of the file, from the base64 it arrived as: four characters carry three bytes.
fn size_of(part: &ContentPart) -> Option<usize> {
    let source = match part {
        ContentPart::Image { source, .. }
        | ContentPart::Audio { source, .. }
        | ContentPart::Video { source, .. } => source,
        _ => return None,
    };
    match source {
        gantry_core::MediaSource::Base64 { data } => Some(data.len() / 4 * 3),
        gantry_core::MediaSource::Blob { .. } => None,
    }
}

#[async_trait]
impl Connector for Media {
    fn descriptor(&self) -> &ConnectorDescriptor {
        &self.descriptor
    }

    async fn tools(&self) -> Result<Vec<ToolDef>, ConnectorError> {
        Ok(definitions())
    }

    /// The model, on the permission card (04 §7).
    ///
    /// The card is where the money is agreed to, so it shows the model that is actually about to
    /// be charged — resolved, not as the chat model typed it — and lets it be changed there.
    /// Before this, a card naming a model the user did not want could only be denied, which cost
    /// a round trip through the chat model to say "use that one instead"; the first live run of
    /// this connector ended exactly that way.
    ///
    /// A model the call named that this machine does not have is **not** silently swapped: the
    /// card shows what it would run instead and says why, and the user sees it before pressing
    /// anything.
    async fn choices(&self, req: &ToolCallRequest) -> Vec<gantry_core::ArgChoice> {
        if req.tool != "generate" {
            return Vec::new();
        }
        let wanted = req
            .args
            .get("kind")
            .and_then(serde_json::Value::as_str)
            .and_then(|k| models::parse_kind(k).ok());
        let named = req
            .args
            .get("model")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|m| !m.is_empty());

        vec![models::model_choice(
            &models::list(&self.providers, &self.store),
            gantry_core::now_ms(),
            &self.preferences(),
            req.scope.mode,
            named,
            wanted,
        )]
    }

    async fn call(
        &self,
        req: ToolCallRequest,
        sink: Arc<dyn ToolEventSink>,
        cancel: CancellationToken,
    ) -> Result<ToolOutcome, ConnectorError> {
        match req.tool.as_str() {
            "generate" => {
                let args = req.args.clone();
                // A clip is minutes of waiting, so Stop stops it: dropping the stream stops the
                // poll loop, because each poll is one step of the stream rather than a task
                // running beside it (02 §4b). `biased` so an already-cancelled token wins
                // deterministically rather than by a coin flip — otherwise a stopped turn still
                // had a chance to start a generation, which is a charge nobody asked for.
                tokio::select! {
                    biased;
                    () = cancel.cancelled() => Ok(ToolOutcome::cancelled()),
                    outcome = self.generate(&args, &req, &sink) => Ok(outcome),
                }
            }
            "list_models" => Ok(self.list_models(&req.args)),
            other => Err(ConnectorError::UnknownTool(other.to_owned())),
        }
    }
}

/// The one tool (03 §3). Not `parallel_safe`: two generations at once are two charges, and the
/// permission card for the second would be answered while the first was already running.
#[must_use]
pub fn definitions() -> Vec<ToolDef> {
    let mut generate = ToolDef::new(
        "generate",
        "Make one picture, one piece of spoken audio, or one video clip, using a model built \
         for it, and put it in your reply. Use this when the user asks for an image, a voice-over, \
         a sound or a clip — not for a diagram, which is an artifact, and not for reading a file. \
         Exactly one thing is made per call: call again for a second. What comes back appears in \
         the answer at this point on its own, so write about it, not a link to it. \
         `kind` says what to make; `model` names a specific one as `provider/model` and can be \
         left out, and the result says which model was used and what it cost. Every call spends \
         money on the user's own account.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "What to make. For `speech` this is the words to read aloud, \
                                    verbatim; for the others it is a description."
                },
                "kind": {
                    "type": "string",
                    "enum": ["image", "speech", "video"],
                    "description": "What kind of thing to make. Required unless `model` names a \
                                    model that only makes one kind."
                },
                "model": {
                    "type": "string",
                    "description": "`provider/model`, e.g. `openrouter/black-forest-labs/flux-1.1-pro`. \
                                    **Leave this out** unless the user named a model or you have just \
                                    read the id from `list_models`: omitted, the call uses the user's \
                                    own default for this kind, and the result says which model that \
                                    was. A name you remember from elsewhere is the one way this call \
                                    fails for free. Models released more than a year ago are not \
                                    yours to pick; the user can still choose one in this \
                                    connector's settings."
                },
                "aspect_ratio": { "type": "string", "description": "`16:9`, `1:1` — only what the model lists." },
                "resolution": { "type": "string", "description": "A video model's own spelling, e.g. `1080p`." },
                "duration_seconds": { "type": "integer", "description": "Seconds of finished video." },
                "quality": { "type": "string", "description": "An image model's quality tier, where it has them." },
                "voice": { "type": "string", "description": "A named voice, for `speech`." }
            },
            "required": ["prompt"]
        }),
        RiskTier::WriteExternal,
    );
    generate.parallel_safe = false;

    // Read, and free: this is the cached catalogue, not a request to anybody. A tool that costs
    // nothing and answers the one question that makes `generate` fail is a tool the model should
    // never have to ask permission to use.
    let list_models = ToolDef::new(
        "list_models",
        "List the media models this machine can actually use: every image, speech and video \
         model on a provider the user has a key for, newest first, with when each was released, \
         what it charges where its provider published that, and the options it offers. Call this \
         instead of guessing a model id — ids you remember from elsewhere are usually not the \
         ones installed here — and call it when the user asks what is available. It reads a \
         local list: no network, no charge, no account touched. The model marked \
         `default_without_a_model` is what `generate` uses when it is not given one. Models over \
         a year old are left out, because they are not yours to pick.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "kind": {
                    "type": "string",
                    "enum": ["image", "speech", "video"],
                    "description": "Only models that make this. Omit for all three."
                },
                "search": {
                    "type": "string",
                    "description": "Words that must all appear in the model id, e.g. `flux` or \
                                    `openrouter video`."
                },
                "limit": {
                    "type": "integer",
                    "description": "How many to report. Default 40, at most 200."
                }
            }
        }),
        RiskTier::Read,
    );
    let mut list_models = list_models;
    // Reading a list beside a generation is two different things happening, not two charges.
    list_models.parallel_safe = true;
    vec![generate, list_models]
}
