# Media generation

Two tools. `media__generate` makes a picture, a piece of spoken audio, or a video clip, and it
appears in the reply where the model asked for it. `media__list_models` says what there is to
make it with.

- **Runs:** in Gantry, in this process. Nothing to install.
- **Needs:** a provider key you already added, on a provider that serves media models — today
  that is OpenRouter's `POST /images`, `/audio/speech` and `/videos` (02 §4b). No second key, no
  account of its own.
- **Can reach:** the media models of the providers you have keys for, and nothing else. It writes
  nothing to this machine.
- **Costs:** every call is a real generation on your own account. A picture is cents; a clip can
  be a dollar or more. The tool is `write_external`, so it asks before each call in Manual and
  Auto-edit, and the card names the model. The result says what it actually cost.

## This is not the media routing

Picking an image model in the model dialog makes that model the one you are talking to: you send
a prompt and it answers with a picture instead of words. That is `02 §4b` and it has been there
since M4. This connector is the other direction — you keep talking to the chat model, and it
calls a drawing model as one step of its own work, so it can say what it is about to make, make
it, and then write about what it made.

Both use the same endpoints and the same client. This connector adds no provider code: it builds
a `ChatRequest` with the prompt as the only message and calls `Provider::stream`, which is where
the routing already lives.

## One generation per call

There is no `n`, no batch and no loop. A model that wants three pictures calls three times, and
each call is one charge on one permission card. Calling it repeatedly is fine and expected; what
is not offered is a single call that spends an amount nobody can see in advance.

The same rule is why the call does not retry: `retries` is 1, so a request that fails is a
request that failed, not a second charge for something asked for once.

## Settings

Open the connector in **Customize → Connectors** and it has a form of its own:

- **A default model per kind** — picture, spoken audio, clip. The menu is the models your own
  provider keys reach, cheapest first, with each one's price. This is what a call gets when it
  does not name a model, which is what it should normally do. "Cheapest available" is the
  starting answer and stays honest as prices change.
- **Most one generation may cost** — a ceiling in dollars. Anything over it is refused before
  the request goes out, and so is a model whose provider publishes no price: a price nobody
  published cannot be shown to be under a ceiling. Leave it empty for no ceiling.

The options are built when the form is opened, from the same catalogue `generate` chooses from,
so the form cannot offer a model the tool would then refuse. The answers are read on every call,
so changing one takes effect on the next generation rather than on the next restart. If a default
you chose stops being available — a key removed, a model retired — the cheapest answers instead
and the result says so, rather than quietly billing you for a different model.

## Where the picture goes

Two places, on purpose.

The **result** the model reads is a sentence and some numbers: which model answered, what kind of
file, how big, what it cost. The bytes are not in it. A picture in a tool result is stored twice —
once in the call row, once in the transcript — and read whole out of SQLite every time the chat is
opened, which is the same argument 02 §4b makes about media models' own answers.

The **file** goes to the answer, through `ToolOutcome::media` (03 §4). The turn loop appends it as
an assistant message of its own, so it renders at the point of the call, its bytes are parked in
the blob store, the sweeper can reach it, and no provider replays it back to the chat model.

One consequence worth knowing: the model cannot look at what it made. It knows the picture exists
and what it asked for, and that is all. The system addendum tells it so, because a model that
thinks it can check its own work will claim to have done it.

## Looking before guessing

`media__list_models` reports every media model on a provider you have a key for, cheapest first,
with the price and the options each one offers — the aspect ratios, resolutions, lengths,
qualities and voices `generate` would otherwise refuse a call for. It reads the cached model list
(02 §2): no network, no charge, nothing touched. It is `read`, so no mode asks about it.

It exists because the failure it prevents is a specific one. A chat model asked for a picture
reaches for a model id it remembers — and an id remembered from training is exactly the kind of
thing that has been renamed since. Refusing that call is right, but a refusal that ends there
costs a round trip and usually ends with the model asking *you* which model to use. So every
refusal about a model name now ends by naming this tool, and the row a call with no `model` would
have got is marked `default_without_a_model`.

## Choosing the model

`model` is `provider/model` and may be left out. Left out, the connector takes the **cheapest**
model of the requested kind among providers with a key, and the result names it — a model with no
published price is never the automatic choice, because an unknown price is the one that cannot be
defended afterwards. A bare model id is accepted when only one provider has it.

Whatever you chose for that model in the model dialog — the voice, the aspect ratio, the length —
is used here too (`ChatSettings.model_options`, 11 §1), with anything the call names on top. An
option the model does not offer is refused before the request is sent, naming what it does offer,
rather than being sent and charged for.

## Verified

The endpoints, the polling and the blob parking are M4's and have their own tests. What is new
here is tested offline: model choice and its refusals, option validation, the one-request rule,
and the answer shape (`tests/generate.rs`). **Nobody has run a live generation from this
connector** — it spends money, and Olav's checklist is where that happens.
