# Third-party crates

Direct dependencies of the engine workspace (`crates/`). `cargo deny check licenses` enforces the
license policy in `deny.toml` for the whole dependency tree; this list is the human summary.

| Crate | Version | License | Used by | For |
|---|---|---|---|---|
| anstream | 1.0 | MIT OR Apache-2.0 | euc-cli | color output that degrades on non-terminals |
| anstyle | 1.0 | MIT OR Apache-2.0 | euc-cli | terminal styles |
| anyhow | 1.0 | MIT OR Apache-2.0 | euc-cli | error reporting in the binary |
| clap | 4.6 | MIT OR Apache-2.0 | euc-cli | command-line parsing |
| crc32fast | 1.5 | MIT OR Apache-2.0 | euc-image | CRC-32/ISO-HDLC of sealed images |
| directories | 6.0 | MIT OR Apache-2.0 | euc-fwsource | per-user cache folder |
| hex | 0.4 | MIT OR Apache-2.0 | euc-profile | sha256 text form |
| object | 0.40 | Apache-2.0 OR MIT | euc-image | reading firmware ELF files |
| serde | 1.0 | MIT OR Apache-2.0 | all but euc-core | (de)serialization |
| serde_json | 1.0 | MIT OR Apache-2.0 | euc-fwsource, euc-cli | vendor API replies, `--json` output |
| sha2 | 0.11 | MIT OR Apache-2.0 | euc-profile | sha256 pins |
| thiserror | 2.0 | MIT OR Apache-2.0 | library crates | typed errors |
| toml | 1.1 | MIT OR Apache-2.0 | euc-profile | wheel profiles |
| ureq | 3.4 | MIT OR Apache-2.0 | euc-fwsource | HTTPS downloads (rustls) |
| tempfile | 3.27 | MIT OR Apache-2.0 | tests only | scratch folders |

Notable licenses deeper in the tree, all allowed by `deny.toml`:

| Crate | License | Pulled in by |
|---|---|---|
| ring | Apache-2.0 AND ISC | ureq → rustls (crypto) |
| rustls-webpki, untrusted | ISC | ureq → rustls (certificate checks) |
| webpki-roots | CDLA-Permissive-2.0 | ureq (Mozilla root certificates) |
| subtle | BSD-3-Clause | sha2 |
| option-ext | MPL-2.0 | directories |
| unicode-ident | (MIT OR Apache-2.0) AND Unicode-3.0 | proc macros |
