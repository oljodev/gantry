# Cloudinary

Cloudinary's asset-management server: search assets, read their metadata, organise folders and
tags.

- **Runs:** nothing locally. `https://asset-management.mcp.cloudinary.com/mcp` — Cloudinary runs several servers and this is the asset one.
- **Needs:** a Cloudinary account; sign-in happens in your browser and the server registers Gantry on the spot.
- **Can reach:** the media library of the account you sign in to. The server offers `asset_management`, `upload` and `media_generation`; **this entry asks for the first**, so a chat can find and describe media without uploading or generating any.
- **Protocol:** verified mechanically when this entry was written (2026-09-12): `401` with a protected-resource document listing those scopes, dynamic registration offered. The tool list is discovered at the first connection; nobody here has signed in, so this entry does not claim to know what it contains.
