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
| 0.1.0 | 2026-09-22 | 2028-09-22 |

## What else is in this repository

- `desktop/connectors/`: first-party connectors are under the same licence. Manifests for third-party
  MCP servers only *describe* servers that have their own licences; nothing of theirs is
  included.
- `desktop/schemas/` and `desktop/connectors/README.md`: the connector folder contract and the manifest schema
  are additionally offered under the MIT licence, so anyone can ship Gantry-compatible connectors
  without touching the FSL. (Formalised before the first public release.)
- `web/site/` and `web/client-metadata/`: the public sites, same licence as the app.
- Third-party components are listed with their licences in `THIRD_PARTY_LICENSES.md`
  (generated at release time).

## Before the first public release

The licensor is **Olav Jodal**, named in the copyright notice of [`LICENSE`](LICENSE) since
2026-09-11. v0.1.0, released 2026-09-22, is the first row of the conversion table above.

## Contributing

Contributions are accepted under the repository licence with a Developer Certificate of Origin
sign-off; see [`CONTRIBUTING.md`](CONTRIBUTING.md). The FSL's future-licence grant means every
contribution also becomes Apache 2.0 on the same schedule.
