# 08 — License plan

## Recommendation: FSL-1.1-ALv2

Use the **Functional Source License, version 1.1, with the Apache 2.0 future license** (SPDX: `FSL-1.1-ALv2`). It fits the brief exactly:

- Free use, self-hosting, internal and private use including inside companies, and full source visibility are all explicitly permitted.
- The one thing withheld is a **Competing Use**: making the software available to others in a commercial product or service that substitutes for Gantry, substitutes for another product the licensor offers using Gantry, or offers the same or substantially similar functionality.
- Each released version converts automatically to **Apache 2.0 two years** after it is first made available.

Why FSL over BSL 1.1:

| | FSL 1.1 | BSL 1.1 |
|--|---------|---------|
| Text you must draft | None; the permitted-purpose and competing-use language is fixed | An **Additional Use Grant** (what production use is allowed) and the choice of Change License; every BSL deployment is effectively its own license |
| Change period | Fixed at 2 years | Up to 4 years, your choice |
| Change license | Apache 2.0 or MIT | Anything you name (GPL, Apache, …) |
| Default production use | Allowed unless it is a Competing Use | **Not** allowed unless the Additional Use Grant says so |
| Precedent for desktop apps | GitButler (desktop Git client) uses FSL | Mostly infrastructure vendors |
| Reading cost for users | Short, standardized, recognized by SPDX | Requires reading the grant to know what is allowed |

FSL was written specifically to remove BSL's variability. For a solo-developer desktop app whose commercial angle is a hosted or bundled future product rather than a database service, the shorter conversion window is not a meaningful loss, and the fixed wording is a real gain: no lawyer time spent inventing a grant, and no ambiguity for users about internal use. If a four-year window ever matters more than simplicity, BSL is the fallback and the grant text below is ready.

## The `LICENSE` file

Use the verbatim template from fsl.software (`FSL-1.1-ALv2.template.md`) with only the two placeholders filled in. The structure, so nothing surprises you:

```
Functional Source License, Version 1.1, Apache License 2.0 Future License

Abbreviation            FSL-1.1-ALv2
Notice                  Copyright 2026 <licensor name>
Terms and Conditions
  Licensor ("We")       the party offering the Software under these terms
  The Software          each version made available under these terms
  License Grant         worldwide, non-exclusive, royalty-free, non-transferable, non-sublicensable
                        license to use, copy, modify, create derivative works, redistribute — for any Permitted Purpose
  Permitted Purpose     any purpose other than a Competing Use; expressly includes internal use and access,
                        non-commercial education, non-commercial research, professional services provided to a licensee
  Competing Use         making the Software available to others in a commercial product or service that
                        (1) substitutes for the Software, (2) substitutes for another product or service we offer
                        using the Software that exists when we make the Software available, or (3) offers the same
                        or substantially similar functionality
  Patents               patent license limited to Permitted Purposes; terminates if you claim the Software infringes
  Redistribution        keep these terms and the notice with copies; derivative works under the same terms
  Disclaimer            as-is, no warranty, no liability for indirect or consequential damages
  Trademarks            no trademark rights granted
Grant of Future License on the second anniversary of first availability of each version, Apache 2.0 applies
```

Placeholders: `${year}` → 2026 (update per release year), `${licensor name}` → the legal licensor (personal name now; a company later means a re-issue for new versions, not a change to old ones). **As built:** `Copyright 2026 Olav Jodal`, set 2026-09-11 before the repository goes public. Do not edit any other sentence; the value of FSL is that it is recognizable text.

Add `SPDX-License-Identifier: FSL-1.1-ALv2` headers to source files through the formatter template, and set `"license": "FSL-1.1-ALv2"` in `package.json` and `Cargo.toml`.

## Companion files

- **`LICENSING.md`** — plain-language summary and FAQ (not legal text): you may use Gantry at work; you may modify it for yourself or your company; you may fork it and share patches; you may not sell or host a Gantry-derived product that competes with Gantry; every version becomes Apache 2.0 two years after its release, with a table of release dates and conversion dates; connectors in this repository are under the same license; the connector *manifests* only describe third-party servers that have their own licenses.
- **`CONTRIBUTING.md`** — contributions are accepted under the repository license with a Developer Certificate of Origin sign-off (no CLA). State plainly that the licensor may relicense (the FSL grant of future license makes this expectation explicit anyway).
- **`THIRD_PARTY_LICENSES.md`** — generated from `cargo about` and `pnpm licenses`; bundled connector icons and any vendor marks are attributed here.
- **`TRADEMARK.md`** (later) — FSL grants no trademark rights; when the name and logo exist, a short policy on using "Gantry" for forks avoids arguments.

## If BSL 1.1 is chosen instead

The parameters block a lawyer needs:

```
Licensor:             <licensor name>
Licensed Work:        Gantry <version>. The Licensed Work is (c) 2026 <licensor name>.
Additional Use Grant: You may make production use of the Licensed Work, including inside a business,
                      provided such use does not include offering the Licensed Work, or a derivative of it,
                      to third parties as a commercial product or hosted service that competes with the
                      Licensor's products built on the Licensed Work.
Change Date:          four years from the release date of this version (state the absolute date per release)
Change License:       Apache License, Version 2.0
```

The Additional Use Grant should be written as a permission with one carve-out, as above; a list of forbidden activities is harder to interpret and ages badly.

## Things to decide before the first public release

1. Licensor identity (personal name or an entity). It affects the Notice line and who can enforce.
2. Whether any component should be more permissive from day one (for example the manifest schema and the connector folder contract under Apache 2.0 or MIT, so third parties can ship Gantry-compatible connectors without touching FSL). Recommended: yes, `schemas/` and `connectors/README.md` under MIT.
3. The public domain used for the OAuth client-metadata document, since that URL is also the client's identity to every authorization server.
