# Licensing, in plain language

Gantry is **source-available** under the Functional Source License, version 1.1, with the
Apache 2.0 future license (`FSL-1.1-ALv2`). The legal text is in [`LICENSE`](LICENSE); this page
is a summary and is not the licence.

## What you may do

- Use Gantry for anything, at home or at work, inside a company or on your own.
- Read, modify and build the source, for yourself or your organisation.
- Fork it, share patches, and redistribute copies or modified versions under the same terms.
- Use it for education, research, and professional services you provide to others.

## The one thing you may not do

Offer Gantry, or something derived from it, to others as a **commercial product or service that
competes with Gantry** or with a product built on it by the licensor. That is the FSL's
"Competing Use". Internal use inside your company is not a competing use.

## It becomes Apache 2.0

Two years after each version is first released, that version converts automatically to the
Apache License 2.0. Every release lists its own conversion date here:

| Version | Released | Becomes Apache 2.0 |
|---------|----------|--------------------|
| (none yet) | | |

## What else is in this repository

- `connectors/`: first-party connectors are under the same licence. Manifests for third-party
  MCP servers only *describe* servers that have their own licences; nothing of theirs is
  included.
- `schemas/` and `connectors/README.md`: the connector folder contract and the manifest schema
  are additionally offered under the MIT licence, so anyone can ship Gantry-compatible connectors
  without touching the FSL. (Formalised before the first public release.)
- `website/` and `client-metadata/`: the public sites, same licence as the app.
- Third-party components are listed with their licences in `THIRD_PARTY_LICENSES.md`
  (generated at release time).

## Before the first public release

The copyright notice in `LICENSE` still carries a placeholder for the licensor's name. It is
filled in, and this table gets its first row, as part of the v0.1.0 release checklist.

## Contributing

Contributions are accepted under the repository licence with a Developer Certificate of Origin
sign-off; see [`CONTRIBUTING.md`](CONTRIBUTING.md). The FSL's future-licence grant means every
contribution also becomes Apache 2.0 on the same schedule.
