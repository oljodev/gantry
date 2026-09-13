# Blender

A community server — not Blender's own — that drives a running Blender from a conversation:
read the scene, make and move geometry, apply materials, fetch assets, take a viewport
screenshot.

- **Runs:** `uvx blender-mcp` on this machine, which needs **uv**. Gantry checks for it before
  the install goes through (03 §11 step 1) and whatever the process writes to stderr is kept
  behind **Show log** on its page.
- **Needs:** the other half, inside Blender. `uvx blender-mcp install-addon` copies the add-on,
  then Edit → Preferences → Add-ons → enable **Interface: MCP for Blender**, then start its
  server from the sidebar. It listens on `127.0.0.1:9876` and the two halves find each other
  there. Blender 3.0 or newer. No account and no key for the core tools; Sketchfab, Poly Pizza,
  Hyper3D Rodin and Hunyuan3D each want their own key, and those are entered in Blender's add-on
  preferences rather than here — Gantry holds one credential per connector and this server holds
  none of them itself.
- **Can reach:** the Blender you have open, completely. `execute_blender_code` is arbitrary
  Python inside that process, which is the point of the project rather than an oversight; the
  asset tools also reach Poly Haven, Sketchfab, Poly Pizza and the two generation services over
  the internet, and download files to this machine.
- **Protocol:** the package was checked against PyPI when this entry was written (2026-09-13,
  version 1.9.1, MIT). The tool list here was read from the published wheel, not from a running
  server — nobody here has spawned it or opened Blender against it.

## Two things this entry decides for you

Both are literal values in `runtime.env`, not settings, because both of these environment
variables fail *open*: anything the server cannot read as a yes is treated as a no, so a setting
that went missing would silently turn the protection off.

**`BLENDER_MCP_DISABLE_TELEMETRY=1`.** The server collects telemetry and it is on by default:
`enabled: bool = True` in `config.py`, and the add-on's consent checkbox ships ticked. What it
uploads is not counters — `telemetry.py` sends the prompt text (to 1,000 characters), the tool
called, errors, platform, Blender version and a persistent install id to its author's Supabase
project, and `_upload_image` puts viewport screenshots in a storage bucket beside them. A user
who wants to contribute that can say so outside Gantry; a user who installs a 3D connector has
not agreed to send their prompts and screenshots to a stranger, and defaulting the other way
would make that decision for them quietly.

**`BLENDER_MCP_SAFE_MODE=1`.** An AST allowlist the server applies to code before it crosses the
socket: interpreter escapes, `os`/`sys`/`subprocess`/`socket`, handlers, timers, drivers and
`bpy.ops.script.*` are refused; rendering, saving and opening `.blend` files, and every
import/export operator still work, so ordinary modelling is unaffected. Its own docstring names
the threat plainly, and it is Gantry's too: asset names and descriptions from Poly Haven,
Sketchfab and Hyper3D arrive in the model's context, which is a short path from a page somebody
else wrote to code running in your Blender. It guards the MCP path only — the add-on's socket
still accepts a raw `execute_code` from any local process, so this is a guard on what the model
can be talked into, not a sandbox around Blender.

Someone who needs unrestricted scripting can add the server by hand under **Add a server** with
their own environment; this entry is the safe one.

## Tier overrides

The default for a local server is `execute`, because a child process can do whatever you can.
Three groups differ:

- **Sixteen `get_*`, `search_*` and `poll_*` tools are `read`.** They inspect a scene or ask an
  asset library a question. Left at `execute`, every look at the scene would raise a permission
  card and the connector would be unusable in Manual mode.
- **`execute_blender_code` is `destructive` with `always_confirm`.** Arbitrary Python in the
  user's Blender process, and the upstream README's own advice is to save your work first. Safe
  mode narrows what it can be, not what it is, and safe mode is not something the tier can
  assume.
- **`generate_hyper3d_model_via_text`, `..._via_images` and `generate_hunyuan3d_model` are
  `destructive` with `always_confirm`.** They spend the user's credits on a paid service, which
  17 §6's last row confirms every time whatever the verb is.
- **`record_trajectory_feedback` is `write_external` with `always_confirm`**, because it uploads
  to the author's project. With telemetry off it should do nothing; a tool whose whole job is to
  send something somewhere still asks first.

The downloads (`download_polyhaven_asset`, `download_polypizza_model`,
`download_sketchfab_model`), the imports and `set_texture` keep the `execute` default. Each
writes a file to this machine and changes the open scene, and nothing here is confident enough
to loosen that.
