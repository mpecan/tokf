# Changelog

## [0.2.26](https://github.com/mpecan/tokf/compare/tokf-v0.2.25...tokf-v0.2.26) (2026-03-06)


### Features

* **cli:** add `tokf telemetry status` subcommand ([#87](https://github.com/mpecan/tokf/issues/87)) ([#254](https://github.com/mpecan/tokf/issues/254)) ([f7138c1](https://github.com/mpecan/tokf/commit/f7138c19c4390976641107ff4de7ab8c68d74aa3))
* **cli:** add reduction stats to tokf verify ([#250](https://github.com/mpecan/tokf/issues/250)) ([ff6bd23](https://github.com/mpecan/tokf/commit/ff6bd230ef9a2059d6bcf0c670a4e6f52ca9a9ac))
* **filter:** condense cargo clippy error output with grouping ([#256](https://github.com/mpecan/tokf/issues/256)) ([5916a80](https://github.com/mpecan/tokf/commit/5916a8045dc3d4cdd0fe49eef9d1393524695425))
* **filter:** JSON extraction via JSONPath (RFC 9535) ([#255](https://github.com/mpecan/tokf/issues/255)) ([dd2759b](https://github.com/mpecan/tokf/commit/dd2759b615f6d54212c2be501c5f27c5593db95e))
* **telemetry:** OpenTelemetry OTLP metrics exporter ([#85](https://github.com/mpecan/tokf/issues/85)) ([#134](https://github.com/mpecan/tokf/issues/134)) ([9b7303d](https://github.com/mpecan/tokf/commit/9b7303dd96b15b4301797a9ee1596b0c294c6e90))
* **tracking:** track raw_bytes, enhance gain display, fix passthrough filters ([#257](https://github.com/mpecan/tokf/issues/257)) ([718552c](https://github.com/mpecan/tokf/commit/718552c31b715de05024b8b89c6274223da64830))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * tokf-common bumped from 0.2.25 to 0.2.26
    * tokf-filter bumped from 0.2.25 to 0.2.26

## [0.2.25](https://github.com/mpecan/tokf/compare/tokf-v0.2.24...tokf-v0.2.25) (2026-03-04)


### Features

* **cli:** improve search ergonomics ([#248](https://github.com/mpecan/tokf/issues/248)) ([a6bc402](https://github.com/mpecan/tokf/commit/a6bc402ec19e2ff0918ed1314ce541a18db79608))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * tokf-common bumped from 0.2.24 to 0.2.25
    * tokf-filter bumped from 0.2.24 to 0.2.25

## [0.2.24](https://github.com/mpecan/tokf/compare/tokf-v0.2.23...tokf-v0.2.24) (2026-03-04)


### Features

* **filter:** add eslint, prettier, and ruff filters ([#164](https://github.com/mpecan/tokf/issues/164)) ([2771860](https://github.com/mpecan/tokf/commit/27718600f8a4e0ca08e8e050b36209777d18ea34))
* **filter:** add go/test filter (issue [#42](https://github.com/mpecan/tokf/issues/42)) ([#246](https://github.com/mpecan/tokf/issues/246)) ([ffdcd30](https://github.com/mpecan/tokf/commit/ffdcd304b9ec8bca44464619f88c362d8a94cd72))
* **filter:** add playwright, vue-tsc, firebase deploy, and vite/build filters ([#165](https://github.com/mpecan/tokf/issues/165)) ([ac4f49e](https://github.com/mpecan/tokf/commit/ac4f49e93edd0f663d0a33ca08c9c683a131a76b))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * tokf-common bumped from 0.2.23 to 0.2.24
    * tokf-filter bumped from 0.2.23 to 0.2.24

## [0.2.23](https://github.com/mpecan/tokf/compare/tokf-v0.2.22...tokf-v0.2.23) (2026-03-04)


### Miscellaneous

* **tokf:** Synchronize workspace versions


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * tokf-common bumped from 0.2.22 to 0.2.23
    * tokf-filter bumped from 0.2.22 to 0.2.23

## [0.2.22](https://github.com/mpecan/tokf/compare/tokf-v0.2.21...tokf-v0.2.22) (2026-03-04)


### Miscellaneous

* **tokf:** Synchronize workspace versions


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * tokf-common bumped from 0.2.21 to 0.2.22
    * tokf-filter bumped from 0.2.21 to 0.2.22

## [0.2.21](https://github.com/mpecan/tokf/compare/tokf-v0.2.20...tokf-v0.2.21) (2026-03-04)


### Features

* **cli:** add rich color for gain, with NO_COLOR support ([#240](https://github.com/mpecan/tokf/issues/240)) ([e9e3e58](https://github.com/mpecan/tokf/commit/e9e3e582c34532c812c376010299c97350dfb8c1))
* **safety:** add filter examples generation and safety checks ([#241](https://github.com/mpecan/tokf/issues/241)) ([2faaa60](https://github.com/mpecan/tokf/commit/2faaa60d867ab49f7312866fcb880031d543992d))


### Bug Fixes

* **cli:** add explicit type annotation for stdlib-publish collect ([#236](https://github.com/mpecan/tokf/issues/236)) ([145ca16](https://github.com/mpecan/tokf/commit/145ca165991ced01f1c1939127ecb5a16daf6a99))


### Performance Improvements

* **cli:** optimize test execution and eliminate unsafe env var access ([#195](https://github.com/mpecan/tokf/issues/195)) ([#237](https://github.com/mpecan/tokf/issues/237)) ([389f52e](https://github.com/mpecan/tokf/commit/389f52efc8e0cdead626fbe10b03a4d2f7617cc4))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * tokf-common bumped from 0.2.20 to 0.2.21
    * tokf-filter bumped from 0.2.20 to 0.2.21

## [0.2.20](https://github.com/mpecan/tokf/compare/tokf-v0.2.19...tokf-v0.2.20) (2026-03-03)


### Features

* **cli:** add `tokf completions` subcommand ([#233](https://github.com/mpecan/tokf/issues/233)) ([3c9bef6](https://github.com/mpecan/tokf/commit/3c9bef65621773156e6fb0085afe7d746876322d))
* **cli:** add onboarding flow and usage stats opt-in ([#235](https://github.com/mpecan/tokf/issues/235)) ([3aeec4b](https://github.com/mpecan/tokf/commit/3aeec4b1a7316f96a2dca51d80549f57ebc34b26))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * tokf-common bumped from 0.2.19 to 0.2.20
    * tokf-filter bumped from 0.2.19 to 0.2.20

## [0.2.19](https://github.com/mpecan/tokf/compare/tokf-v0.2.18...tokf-v0.2.19) (2026-03-02)


### Code Refactoring

* **cli,server:** consolidate publish and stdlib-publish paths ([#231](https://github.com/mpecan/tokf/issues/231)) ([1ecc3fb](https://github.com/mpecan/tokf/commit/1ecc3fb6c009fdebaf1c16592179fe7fc55fec36))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * tokf-common bumped from 0.2.18 to 0.2.19
    * tokf-filter bumped from 0.2.18 to 0.2.19

## [0.2.18](https://github.com/mpecan/tokf/compare/tokf-v0.2.17...tokf-v0.2.18) (2026-03-02)


### Bug Fixes

* **server,cli:** use i64 for ToS version fields (CockroachDB INT8) ([#229](https://github.com/mpecan/tokf/issues/229)) ([df084d4](https://github.com/mpecan/tokf/commit/df084d4e8662a6e7431f387b13cdc08e9ecbdb45))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * tokf-common bumped from 0.2.17 to 0.2.18
    * tokf-filter bumped from 0.2.17 to 0.2.18

## [0.2.17](https://github.com/mpecan/tokf/compare/tokf-v0.2.16...tokf-v0.2.17) (2026-03-02)


### Miscellaneous

* **tokf:** Synchronize workspace versions


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * tokf-common bumped from 0.2.16 to 0.2.17
    * tokf-filter bumped from 0.2.16 to 0.2.17

## [0.2.16](https://github.com/mpecan/tokf/compare/tokf-v0.2.15...tokf-v0.2.16) (2026-03-02)


### Features

* **history:** add `tokf history last` subcommand ([#223](https://github.com/mpecan/tokf/issues/223)) ([83f7c11](https://github.com/mpecan/tokf/commit/83f7c1163954619b140d82ac3d4b198bcee80c9c))
* **server,cli:** add Terms of Service and account deletion ([#224](https://github.com/mpecan/tokf/issues/224)) ([468e93c](https://github.com/mpecan/tokf/commit/468e93c33eefd2e91bf466b5ac4e4766f2133827))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * tokf-common bumped from 0.2.15 to 0.2.16
    * tokf-filter bumped from 0.2.15 to 0.2.16

## [0.2.15](https://github.com/mpecan/tokf/compare/tokf-v0.2.14...tokf-v0.2.15) (2026-03-02)


### Bug Fixes

* **config:** normalize basename on both pattern and input words ([#221](https://github.com/mpecan/tokf/issues/221)) ([a236a8b](https://github.com/mpecan/tokf/commit/a236a8bd575eec760bfba3f4e15218b8c084eca7))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * tokf-common bumped from 0.2.14 to 0.2.15
    * tokf-filter bumped from 0.2.14 to 0.2.15

## [0.2.14](https://github.com/mpecan/tokf/compare/tokf-v0.2.13...tokf-v0.2.14) (2026-03-02)


### Bug Fixes

* **output:** store savings_pct as 0-100, rename INSTALLS to RUNS ([#217](https://github.com/mpecan/tokf/issues/217)) ([ea2fc11](https://github.com/mpecan/tokf/commit/ea2fc11f200b6b009e0a3894e804b3035f996b2e))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * tokf-common bumped from 0.2.13 to 0.2.14
    * tokf-filter bumped from 0.2.13 to 0.2.14

## [0.2.13](https://github.com/mpecan/tokf/compare/tokf-v0.2.12...tokf-v0.2.13) (2026-03-02)


### Features

* **cli,server:** filter search, download, and install — tokf search/install ([#118](https://github.com/mpecan/tokf/issues/118)) ([#183](https://github.com/mpecan/tokf/issues/183)) ([e0e17c9](https://github.com/mpecan/tokf/commit/e0e17c9b2c45ab6487ccc276a0a6ccc82a44628a))
* **cli:** centralized HTTP client with retry and error classification ([#200](https://github.com/mpecan/tokf/issues/200)) ([b1d676d](https://github.com/mpecan/tokf/commit/b1d676d7c7bd8563f4146aee039d42de1cc909ea))
* **cli:** remote stats sync and gain display ([#115](https://github.com/mpecan/tokf/issues/115)) ([#188](https://github.com/mpecan/tokf/issues/188)) ([3253f2f](https://github.com/mpecan/tokf/commit/3253f2f4d09909abdb4c34ac919405a8022801fc))
* **cli:** shell-override wrappers for task runners (make, just) ([#209](https://github.com/mpecan/tokf/issues/209)) ([3a1cd9a](https://github.com/mpecan/tokf/commit/3a1cd9af6cb348945b6438b882cb4a6c12249ef4))
* **filter,cli,server:** sandbox CLI Lua execution and inline scripts on publish ([#194](https://github.com/mpecan/tokf/issues/194)) ([4e104cb](https://github.com/mpecan/tokf/commit/4e104cb00563cdda390b9e9e44663c45e3bd9e7f))
* **filter:** chunk processing engine with tree-structured grouping ([#203](https://github.com/mpecan/tokf/issues/203)) ([87557f5](https://github.com/mpecan/tokf/commit/87557f504f86b5e449adf518d43da03f88a8e1bc))
* **server,cli:** allow updating test suites for published filters ([#119](https://github.com/mpecan/tokf/issues/119)) ([#192](https://github.com/mpecan/tokf/issues/192)) ([ef9428b](https://github.com/mpecan/tokf/commit/ef9428b06d3d13ca4d8a2a422b3188bf3bbe5914))
* **server,cli:** comprehensive API rate limiting ([#196](https://github.com/mpecan/tokf/issues/196)) ([52592a5](https://github.com/mpecan/tokf/commit/52592a509dbe7b2b457351fad093592d4d077e33))
* **server,cli:** publish stdlib filters to registry ([#204](https://github.com/mpecan/tokf/issues/204)) ([#205](https://github.com/mpecan/tokf/issues/205)) ([e9bbb61](https://github.com/mpecan/tokf/commit/e9bbb61e59ae795e65606b6b6d50c374adec7965))
* **tracking,cli,server:** usage stats sync and aggregation endpoints ([#114](https://github.com/mpecan/tokf/issues/114)) ([#186](https://github.com/mpecan/tokf/issues/186)) ([30d4ef2](https://github.com/mpecan/tokf/commit/30d4ef28dc4a4f56418b997c910486c4280dc28b))


### Bug Fixes

* **cli,server:** use test: prefix in multipart fields and add just recipes ([#193](https://github.com/mpecan/tokf/issues/193)) ([737d522](https://github.com/mpecan/tokf/commit/737d522e918bcab0df8bc0e172a17291b18edc66))
* **hook:** use bare tokf in generated hook scripts ([#208](https://github.com/mpecan/tokf/issues/208)) ([08a47d4](https://github.com/mpecan/tokf/commit/08a47d42087c23f0b23bcd1fe903b4fdbf3408e3))


### Code Refactoring

* **cli:** address Copilot review feedback ([#201](https://github.com/mpecan/tokf/issues/201)) ([e39ccf7](https://github.com/mpecan/tokf/commit/e39ccf7297a30dfafa28ced73ef836bc6b1a6fea))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * tokf-common bumped from 0.2.12 to 0.2.13
    * tokf-filter bumped from 0.2.12 to 0.2.13

## [0.2.12](https://github.com/mpecan/tokf/compare/tokf-v0.2.11...tokf-v0.2.12) (2026-02-26)


### Features

* **cli,server:** filter publishing — tokf publish &lt;filter-name&gt; ([#117](https://github.com/mpecan/tokf/issues/117)) ([#181](https://github.com/mpecan/tokf/issues/181)) ([acf495f](https://github.com/mpecan/tokf/commit/acf495f08f35fb54c9ec8a488c6d4010c33a02d1))
* **filter:** rewrite git/status and add cargo/fmt stdlib filters ([#184](https://github.com/mpecan/tokf/issues/184)) ([06d41c3](https://github.com/mpecan/tokf/commit/06d41c3f78acbd3e442b57764e48aa9461a2f4fe))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * tokf-common bumped from 0.2.11 to 0.2.12

## [0.2.11](https://github.com/mpecan/tokf/compare/tokf-v0.2.10...tokf-v0.2.11) (2026-02-26)


### Features

* **ci:** sticky PR comment for filter verification + reject empty fixtures ([#173](https://github.com/mpecan/tokf/issues/173)) ([c66af9e](https://github.com/mpecan/tokf/commit/c66af9edd60fa4196fc9b2b4753ea1271ebd0b1e))
* **cli,server:** machine UUID registration for remote sync ([#113](https://github.com/mpecan/tokf/issues/113)) ([#179](https://github.com/mpecan/tokf/issues/179)) ([8535a85](https://github.com/mpecan/tokf/commit/8535a85eef08124e853e478965260811ddd1dec5))
* **cli:** add tokf auth login/logout/status commands ([#178](https://github.com/mpecan/tokf/issues/178)) ([92bcf6b](https://github.com/mpecan/tokf/commit/92bcf6b3cf60dc6f26adb7881c6d02ddf776c260))
* **cli:** add TOKF_HOME env var and improve permission diagnostics ([#180](https://github.com/mpecan/tokf/issues/180)) ([10d4d37](https://github.com/mpecan/tokf/commit/10d4d377dd8512dbc78abe1b2ff6f055c5291551))


### Bug Fixes

* **ci:** stop running stdlib verify tests redundantly in cargo test ([#175](https://github.com/mpecan/tokf/issues/175)) ([52b4f9a](https://github.com/mpecan/tokf/commit/52b4f9aa847097cf186d06e94b9d7feced34047d))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * tokf-common bumped from 0.2.10 to 0.2.11

## [0.2.10](https://github.com/mpecan/tokf/compare/tokf-v0.2.9...tokf-v0.2.10) (2026-02-25)


### Features

* **filter:** add --preserve-color flag for ANSI color passthrough ([#162](https://github.com/mpecan/tokf/issues/162)) ([4187493](https://github.com/mpecan/tokf/commit/4187493fbeabe423100ad7bd58fbce0b8726a8df))


### Code Refactoring

* split oversized files, reduce duplication, add cargo-dupes CI ([#161](https://github.com/mpecan/tokf/issues/161)) ([d269603](https://github.com/mpecan/tokf/commit/d2696039c71f9305e915cb18325650e7d465347e))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * tokf-common bumped from 0.2.9 to 0.2.10

## [0.2.9](https://github.com/mpecan/tokf/compare/tokf-v0.2.8...tokf-v0.2.9) (2026-02-24)


### Features

* **cli:** add --raw flag to history show ([#155](https://github.com/mpecan/tokf/issues/155)) ([a98c34d](https://github.com/mpecan/tokf/commit/a98c34d89c7ef09c3305720544beebbef1258fe9))
* **cli:** add tokf info command and tokf verify --scope ([#158](https://github.com/mpecan/tokf/issues/158)) ([7263e30](https://github.com/mpecan/tokf/commit/7263e307441b70116a543d0fa27bdb0f276c1f88))
* **hook:** add OpenAI Codex CLI integration ([#157](https://github.com/mpecan/tokf/issues/157)) ([a837661](https://github.com/mpecan/tokf/commit/a8376616fdf8e6ce59c0377170120a6abc4dafb5))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * tokf-common bumped from 0.2.8 to 0.2.9

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
