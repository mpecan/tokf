# Changelog

## [0.2.8](https://github.com/mpecan/tokf/compare/tokf-v0.2.7...tokf-v0.2.8) (2026-02-24)


### Features

* **cli:** pipe stripping control, --prefer-less mode, and override tracking ([#154](https://github.com/mpecan/tokf/issues/154)) ([7f24f12](https://github.com/mpecan/tokf/commit/7f24f12deb3b6968a9a857b9a6f327c7796928aa))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * tokf-common bumped from 0.2.7 to 0.2.8

## [0.2.7](https://github.com/mpecan/tokf/compare/tokf-v0.2.6...tokf-v0.2.7) (2026-02-24)


### Features

* **cli:** exit-code masking and improved push filter ([#150](https://github.com/mpecan/tokf/issues/150)) ([1b97ce5](https://github.com/mpecan/tokf/commit/1b97ce5f97b1b9ed281f39844131cce8abebc2ec))
* **server:** add DB connection pooling, schema migrations, and health probes ([#140](https://github.com/mpecan/tokf/issues/140)) ([dc4c85a](https://github.com/mpecan/tokf/commit/dc4c85ab1076ea49559d3a1a83c30630e7290547))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * tokf-common bumped from 0.2.6 to 0.2.7

## [0.2.6](https://github.com/mpecan/tokf/compare/tokf-v0.2.5...tokf-v0.2.6) (2026-02-23)


### Features

* **rewrite:** strip leading env var prefix before command matching ([#141](https://github.com/mpecan/tokf/issues/141)) ([4aca301](https://github.com/mpecan/tokf/commit/4aca30114d353bce8db611755c2e04a40df14dfb))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * tokf-common bumped from 0.2.5 to 0.2.6

## [0.2.5](https://github.com/mpecan/tokf/compare/tokf-v0.2.4...tokf-v0.2.5) (2026-02-23)


### Features

* **hook:** add opencode plugin installer ([#136](https://github.com/mpecan/tokf/issues/136)) ([dee751d](https://github.com/mpecan/tokf/commit/dee751d3e2c88afde690c42d97502bb91726b0d0))
* **server:** bootstrap axum server with /health, config, and CI ([#109](https://github.com/mpecan/tokf/issues/109)) ([#127](https://github.com/mpecan/tokf/issues/127)) ([90bcf72](https://github.com/mpecan/tokf/commit/90bcf724872a25038ac8eb37ba37409f4cf73181))


### Bug Fixes

* **skill:** move skill files into crate for cargo package compatibility ([#137](https://github.com/mpecan/tokf/issues/137)) ([da3b653](https://github.com/mpecan/tokf/commit/da3b6531e9d97134d263c305d9470e9102764b67))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * tokf-common bumped from 0.2.4 to 0.2.5

## [0.2.4](https://github.com/mpecan/tokf/compare/tokf-v0.2.3...tokf-v0.2.4) (2026-02-23)


### Miscellaneous

* **tokf:** Synchronize workspace versions


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * tokf-common bumped from 0.2.3 to 0.2.4

## [0.2.3](https://github.com/mpecan/tokf/compare/tokf-v0.2.2...tokf-v0.2.3) (2026-02-23)


### Features

* **filter:** canonical content hash for filter identity ([#126](https://github.com/mpecan/tokf/issues/126)) ([5abfaf8](https://github.com/mpecan/tokf/commit/5abfaf819833eb625eba47c35e947bbfe9540474))
* **filter:** show history hint for filtered output ([#129](https://github.com/mpecan/tokf/issues/129)) ([9eca37c](https://github.com/mpecan/tokf/commit/9eca37ce1ec0ed1cb0dcbf2ac2b899b00db15883))


### Documentation

* document history hint, pipe stripping, and add docs requirement ([#130](https://github.com/mpecan/tokf/issues/130)) ([52c99be](https://github.com/mpecan/tokf/commit/52c99be6028f3e0d6f3b9d9e66ad90ac1a0c0a7a))


### Code Refactoring

* restructure repository as a Cargo workspace ([#124](https://github.com/mpecan/tokf/issues/124)) ([23396d5](https://github.com/mpecan/tokf/commit/23396d50271f0764619f89b302d84443bf1ab32d))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * tokf-common bumped from 0.2.2 to 0.2.3

## [0.2.2](https://github.com/mpecan/tokf/compare/tokf-v0.2.1...tokf-v0.2.2) (2026-02-22)


### Features

* **filter:** support filter variants and delegation ([#104](https://github.com/mpecan/tokf/issues/104)) ([1525976](https://github.com/mpecan/tokf/commit/1525976f83eb499608253052498194b4c679688c))
* **rewrite:** fair accounting for stripped pipes ([#105](https://github.com/mpecan/tokf/issues/105)) ([993120c](https://github.com/mpecan/tokf/commit/993120c8809e164d01d76565b22e11814ec48df5))


### Documentation

* comprehensive pre-release documentation update ([#106](https://github.com/mpecan/tokf/issues/106)) ([3b0ef96](https://github.com/mpecan/tokf/commit/3b0ef96e570186c10a3993b3585da482ceacf605))
* **readme:** document eject subcommand ([#102](https://github.com/mpecan/tokf/issues/102)) ([27479cc](https://github.com/mpecan/tokf/commit/27479cc2f341faa3a12f36d4b6a10e0d9de32b4b))

## [0.2.1](https://github.com/mpecan/tokf/compare/tokf-v0.2.0...tokf-v0.2.1) (2026-02-21)


### Features

* **cli:** add eject subcommand for filter customization ([#100](https://github.com/mpecan/tokf/issues/100)) ([f855dd0](https://github.com/mpecan/tokf/commit/f855dd0f20fecadc59241087ca741f2170ef1de3))
* **filter:** improve weak stdlib filters ([#98](https://github.com/mpecan/tokf/issues/98)) ([c766fc5](https://github.com/mpecan/tokf/commit/c766fc50685b8641230d50e855462e62a8a3f607))


### Documentation

* **readme:** improve discoverability and add before/after examples ([#96](https://github.com/mpecan/tokf/issues/96)) ([4b3d809](https://github.com/mpecan/tokf/commit/4b3d80908d76008e993d7018ae811c620bd68423))

## [0.2.0](https://github.com/mpecan/tokf/compare/tokf-v0.1.8...tokf-v0.2.0) (2026-02-20)


### ⚠ BREAKING CHANGES

* **rewrite:** piped commands (e.g. `cargo test | grep FAILED`) are no longer rewritten by the hook or `tokf rewrite`. Previously they were wrapped with `tokf run`, causing downstream tools to receive filtered rather than raw output. The public `rewrite(command, verbose)` function signature now requires a `verbose: bool` argument.

### Bug Fixes

* **rewrite:** skip auto-rewrite for piped commands ([#93](https://github.com/mpecan/tokf/issues/93)) ([6b2e350](https://github.com/mpecan/tokf/commit/6b2e35012f27d7d295e4c48123e07400706e7019))

## [0.1.8](https://github.com/mpecan/tokf/compare/tokf-v0.1.7...tokf-v0.1.8) (2026-02-20)


### Features

* **cli:** add tokf verify — declarative filter test suites ([#57](https://github.com/mpecan/tokf/issues/57)) ([#61](https://github.com/mpecan/tokf/issues/61)) ([78fc4c9](https://github.com/mpecan/tokf/commit/78fc4c9057ba88288cab7c7c91cf7a7d720f2527))
* **verify:** add --require-all flag to fail on uncovered filters ([#77](https://github.com/mpecan/tokf/issues/77)) ([b7adfd8](https://github.com/mpecan/tokf/commit/b7adfd851de1a8cf4f92b953eb84390770737ceb))


### Bug Fixes

* **deps:** update rust crate toml to 0.9 ([#64](https://github.com/mpecan/tokf/issues/64)) ([091d0ea](https://github.com/mpecan/tokf/commit/091d0ea2c613eae88f762d2b9f2ba297023c06cc))
* **deps:** update rust crate toml to v1 ([#68](https://github.com/mpecan/tokf/issues/68)) ([e0d5745](https://github.com/mpecan/tokf/commit/e0d57451ed3f8d615a7312c2dcadcb45a2d461f1))


### Code Refactoring

* **filter:** replace Rust filter tests with declarative verify suites ([#69](https://github.com/mpecan/tokf/issues/69)) ([46e5591](https://github.com/mpecan/tokf/commit/46e5591c3b93b942ceccae95eecda30e30f60fe3))

## [0.1.7](https://github.com/mpecan/tokf/compare/tokf-v0.1.6...tokf-v0.1.7) (2026-02-20)


### Bug Fixes

* **cli:** add --version flag to tokf ([#58](https://github.com/mpecan/tokf/issues/58)) ([6527cbc](https://github.com/mpecan/tokf/commit/6527cbc896c828406a0c90a6aade06dd7ab077ef))

## [0.1.6](https://github.com/mpecan/tokf/compare/tokf-v0.1.5...tokf-v0.1.6) (2026-02-20)


### Features

* **config:** basename matching + transparent global flag interception ([#55](https://github.com/mpecan/tokf/issues/55)) ([86ee7b5](https://github.com/mpecan/tokf/commit/86ee7b55fb50ad8bd539464ba06495357cfc1e9e))
* **filter:** add docker build and compose filters ([#50](https://github.com/mpecan/tokf/issues/50)) ([44bffe8](https://github.com/mpecan/tokf/commit/44bffe811881548597a12f8f2598232ae5ada15f))
* **filter:** add Gradle build/test/dependencies filters ([#54](https://github.com/mpecan/tokf/issues/54)) ([029cacb](https://github.com/mpecan/tokf/commit/029cacb1a6672fd4fdd71967e50b3bb31aa02c2a))
* **filter:** output cleanup flags — strip_ansi, trim_lines, strip_empty_lines, collapse_empty_lines ([#46](https://github.com/mpecan/tokf/issues/46)) ([#47](https://github.com/mpecan/tokf/issues/47)) ([9bdf69b](https://github.com/mpecan/tokf/commit/9bdf69bd2896d6886bbebf46a09d721dd7239e1b))
* **history:** store raw and filtered outputs for debugging ([#52](https://github.com/mpecan/tokf/issues/52)) ([d193109](https://github.com/mpecan/tokf/commit/d193109a1515a98b50652fa499c67398a00fe393))

## [0.1.5](https://github.com/mpecan/tokf/compare/tokf-v0.1.4...tokf-v0.1.5) (2026-02-19)


### Bug Fixes

* **ci:** explicitly add rustup target before cross-compiling x86_64-apple-darwin ([#34](https://github.com/mpecan/tokf/issues/34)) ([ca91c5e](https://github.com/mpecan/tokf/commit/ca91c5eb9e877667c462875f695274ec98f8564e))

## [0.1.4](https://github.com/mpecan/tokf/compare/tokf-v0.1.3...tokf-v0.1.4) (2026-02-19)


### Bug Fixes

* **ci:** use macos-14 runner for x86_64-apple-darwin cross-compilation ([#32](https://github.com/mpecan/tokf/issues/32)) ([30be4d1](https://github.com/mpecan/tokf/commit/30be4d133a38d67f2c969ce41ff931ea0d01493b))

## [0.1.3](https://github.com/mpecan/tokf/compare/tokf-v0.1.2...tokf-v0.1.3) (2026-02-19)


### Features

* Homebrew installation support ([#30](https://github.com/mpecan/tokf/issues/30)) ([45acd28](https://github.com/mpecan/tokf/commit/45acd2837d79182ec56e238491328b740c0dc286))

## [0.1.2](https://github.com/mpecan/tokf/compare/tokf-v0.1.1...tokf-v0.1.2) (2026-02-19)


### Bug Fixes

* **hook:** shell-quote hook script path in settings.json ([#29](https://github.com/mpecan/tokf/issues/29)) ([7a2eb1b](https://github.com/mpecan/tokf/commit/7a2eb1bc4a7013429d3980b60304b0ab16142eac))


### Documentation

* credit rtk as inspiration in README ([#24](https://github.com/mpecan/tokf/issues/24)) ([99c7099](https://github.com/mpecan/tokf/commit/99c70997fe8944033528a09173664f4da5ded34cb))

## [0.1.1](https://github.com/mpecan/tokf/compare/tokf-v0.1.0...tokf-v0.1.1) (2026-02-19)


### Features

* **cli:** add tokf skill install subcommand ([485fcd2](https://github.com/mpecan/tokf/commit/485fcd200ae4bc446fca3e10f782adf27a1b0df6)), closes [#19](https://github.com/mpecan/tokf/issues/19)


### Documentation

* **filter:** add Claude Code skill for filter authoring ([2882d3a](https://github.com/mpecan/tokf/commit/2882d3a6a7d34ddd66b2fd50ce3b6c2ec6694f8b)), closes [#19](https://github.com/mpecan/tokf/issues/19)
