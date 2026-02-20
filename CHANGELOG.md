# Changelog

## [0.1.7](https://github.com/mpecan/tokf/compare/tokf-v0.1.6...tokf-v0.1.7) (2026-02-20)


### Bug Fixes

* **cli:** add --version flag to tokf ([#58](https://github.com/mpecan/tokf/issues/58)) ([6527cbc](https://github.com/mpecan/tokf/commit/6527cbc896c828406a0c90a6aade06dd7ab077ef))

## [0.1.6](https://github.com/mpecan/tokf/compare/tokf-v0.1.5...tokf-v0.1.6) (2026-02-20)


### Features

* **config:** basename matching + transparent global flag interception ([#55](https://github.com/mpecan/tokf/issues/55)) ([86ee7b5](https://github.com/mpecan/tokf/commit/86ee7b55fb50ad8bd539464ba06495357cfc1e9e))
* **filter:** add docker build and compose filters ([#50](https://github.com/mpecan/tokf/issues/50)) ([44bffe8](https://github.com/mpecan/tokf/commit/44bffe811881548597a12f8f2598232ae5ada15f))
* **filter:** add Gradle build/test/dependencies filters ([#54](https://github.com/mpecan/tokf/issues/54)) ([029cacb](https://github.com/mpecan/tokf/commit/029cacb1a6672fd4fdd71967e50b3bb31aa02c2a))
* **filter:** output cleanup flags — strip_ansi, trim_lines, strip_empty_lines, collapse_empty_lines ([#46](https://github.com/mpecan/tokf/issues/46)) ([#47](https://github.com/mpecan/tokf/issues/47)) ([9bdf69b](https://github.com/mpecan/tokf/commit/9bdf69bd2896d6886bbebf46a09d725dd7239e1b))
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

* **hook:** shell-quote hook script path in settings.json ([f4f07f3](https://github.com/mpecan/tokf/commit/f4f07f3deff490abcff9950702d567e83947b488))
* **hook:** shell-quote hook script path in settings.json ([#29](https://github.com/mpecan/tokf/issues/29)) ([7a2eb1b](https://github.com/mpecan/tokf/commit/7a2eb1bc4a7013429d3980b60304b0ab16142eac))


### Documentation

* credit rtk as inspiration in README ([47668d3](https://github.com/mpecan/tokf/commit/47668d3d42bd004e131bda804da51a12f0b6b9dc)), closes [#19](https://github.com/mpecan/tokf/issues/19)
* credit rtk as inspiration in README ([#24](https://github.com/mpecan/tokf/issues/24)) ([99c7099](https://github.com/mpecan/tokf/commit/99c70997fe8944033528a09173664fda5ded34cb))

## [0.1.1](https://github.com/mpecan/tokf/compare/tokf-v0.1.0...tokf-v0.1.1) (2026-02-19)


### Features

* **cli:** add tokf skill install subcommand ([485fcd2](https://github.com/mpecan/tokf/commit/485fcd200ae4bc446fca3e10f782adf27a1b0df6)), closes [#19](https://github.com/mpecan/tokf/issues/19)


### Documentation

* **filter:** add Claude Code skill for filter authoring ([2882d3a](https://github.com/mpecan/tokf/commit/2882d3a6a7d34ddd66b2fd50ce3b6c2ec6694f8b)), closes [#19](https://github.com/mpecan/tokf/issues/19)
