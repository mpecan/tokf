# Changelog

## [0.2.12](https://github.com/mpecan/tokf/compare/tokf-server-v0.2.11...tokf-server-v0.2.12) (2026-02-26)


### Features

* **cli,server:** filter publishing — tokf publish &lt;filter-name&gt; ([#117](https://github.com/mpecan/tokf/issues/117)) ([#181](https://github.com/mpecan/tokf/issues/181)) ([acf495f](https://github.com/mpecan/tokf/commit/acf495f08f35fb54c9ec8a488c6d4010c33a02d1))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * tokf-common bumped from 0.2.10 to 0.2.12

## [0.2.11](https://github.com/mpecan/tokf/compare/tokf-server-v0.2.10...tokf-server-v0.2.11) (2026-02-26)


### Features

* **cli,server:** machine UUID registration for remote sync ([#113](https://github.com/mpecan/tokf/issues/113)) ([#179](https://github.com/mpecan/tokf/issues/179)) ([8535a85](https://github.com/mpecan/tokf/commit/8535a85eef08124e853e478965260811ddd1dec5))


### Bug Fixes

* **server:** CockroachDB compatibility — SQL fix, CI migration, test macro ([#176](https://github.com/mpecan/tokf/issues/176)) ([507cfd0](https://github.com/mpecan/tokf/commit/507cfd00eae7509687b338f62588fda80760bcdc))
* **server:** disable pg_advisory_lock for CockroachDB compatibility ([#170](https://github.com/mpecan/tokf/issues/170)) ([d1eb68d](https://github.com/mpecan/tokf/commit/d1eb68d49e967aa4aaea34b4119e5f7cf1440dde))

## [0.2.10](https://github.com/mpecan/tokf/compare/tokf-server-v0.2.9...tokf-server-v0.2.10) (2026-02-25)


### Code Refactoring

* split oversized files, reduce duplication, add cargo-dupes CI ([#161](https://github.com/mpecan/tokf/issues/161)) ([d269603](https://github.com/mpecan/tokf/commit/d2696039c71f9305e915cb18325650e7d465347e))

## [0.2.9](https://github.com/mpecan/tokf/compare/tokf-server-v0.2.8...tokf-server-v0.2.9) (2026-02-24)


### Miscellaneous

* **tokf-server:** Synchronize workspace versions

## [0.2.8](https://github.com/mpecan/tokf/compare/tokf-server-v0.2.7...tokf-server-v0.2.8) (2026-02-24)


### Features

* **server:** add Cloudflare R2 blob storage integration ([#149](https://github.com/mpecan/tokf/issues/149)) ([e4bef85](https://github.com/mpecan/tokf/commit/e4bef85153bc7e7070c2de618e92b71081c49f8d))

## [0.2.7](https://github.com/mpecan/tokf/compare/tokf-server-v0.2.6...tokf-server-v0.2.7) (2026-02-24)


### Features

* **server:** add DB connection pooling, schema migrations, and health probes ([#140](https://github.com/mpecan/tokf/issues/140)) ([dc4c85a](https://github.com/mpecan/tokf/commit/dc4c85ab1076ea49559d3a1a83c30630e7290547))
* **server:** add GitHub OAuth device flow endpoints ([#148](https://github.com/mpecan/tokf/issues/148)) ([2c6ad5f](https://github.com/mpecan/tokf/commit/2c6ad5fab019cc771f0159e97b978d6d9663d72d))

## [0.2.6](https://github.com/mpecan/tokf/compare/tokf-server-v0.2.5...tokf-server-v0.2.6) (2026-02-23)


### Miscellaneous

* **tokf-server:** Synchronize workspace versions

## [0.2.5](https://github.com/mpecan/tokf/compare/tokf-server-v0.2.4...tokf-server-v0.2.5) (2026-02-23)


### Features

* **server:** bootstrap axum server with /health, config, and CI ([#109](https://github.com/mpecan/tokf/issues/109)) ([#127](https://github.com/mpecan/tokf/issues/127)) ([90bcf72](https://github.com/mpecan/tokf/commit/90bcf724872a25038ac8eb37ba37409f4cf73181))

## [0.2.4](https://github.com/mpecan/tokf/compare/tokf-server-v0.2.3...tokf-server-v0.2.4) (2026-02-23)


### Bug Fixes

* **config:** add missing publish metadata to crates ([#131](https://github.com/mpecan/tokf/issues/131)) ([c235072](https://github.com/mpecan/tokf/commit/c235072676673e7106018ef819edfb3fdcd32658))

## [0.2.3](https://github.com/mpecan/tokf/compare/tokf-server-v0.2.2...tokf-server-v0.2.3) (2026-02-23)


### Code Refactoring

* restructure repository as a Cargo workspace ([#124](https://github.com/mpecan/tokf/issues/124)) ([23396d5](https://github.com/mpecan/tokf/commit/23396d50271f0764619f89b302d84443bf1ab32d))
