# Changelog

## [0.1.16](https://github.com/teevik/tarkov-map/compare/v0.1.15...v0.1.16) (2026-08-19)


### Features

* add BTR stops overlay ([#67](https://github.com/teevik/tarkov-map/issues/67)) ([e78e9d7](https://github.com/teevik/tarkov-map/commit/e78e9d7c78d6d262443c824cc5ce471626732895))
* add hazard overlays ([#64](https://github.com/teevik/tarkov-map/issues/64)) ([091f91f](https://github.com/teevik/tarkov-map/commit/091f91f1eb1da2dce5670f72986499ad6c2ec40c))
* add per-Mob Boss Spawn overlays ([#68](https://github.com/teevik/tarkov-map/issues/68)) ([4a03a1f](https://github.com/teevik/tarkov-map/commit/4a03a1fb628c1f9feee5baa0430fc1c99469d07f))
* add switches overlay ([#66](https://github.com/teevik/tarkov-map/issues/66)) ([725c8ee](https://github.com/teevik/tarkov-map/commit/725c8ee3eed3c819923a3e49bd4dcaebd1b25225))
* add transit overlay and marker primitives ([#65](https://github.com/teevik/tarkov-map/issues/65)) ([6ad0680](https://github.com/teevik/tarkov-map/commit/6ad06802d604bf1cc24e45b0de2f4a23134f17aa))
* hamburger button in the menu bar to toggle the sidebar ([5e27ea3](https://github.com/teevik/tarkov-map/commit/5e27ea3ee8e616c4060e4ed187802fe5b7969531))
* show boss spawns as inferred areas ([#68](https://github.com/teevik/tarkov-map/issues/68)) ([93d09ab](https://github.com/teevik/tarkov-map/commit/93d09ab9a06b9a9609409aa6bc24b4196240c580))


### Bug Fixes

* more readable font sizes ([045b5b8](https://github.com/teevik/tarkov-map/commit/045b5b884f613dbc94db12ec1fcf26bd5221d240))
* union same-type hazard outlines before painting ([3e582c5](https://github.com/teevik/tarkov-map/commit/3e582c531705adc4eb9b61de93ba10d59a53ca30))

## [0.1.15](https://github.com/teevik/tarkov-map/compare/v0.1.14...v0.1.15) (2026-08-19)


### Features

* bundle richer overlay data ([#61](https://github.com/teevik/tarkov-map/issues/61)) ([706b818](https://github.com/teevik/tarkov-map/commit/706b818d79dce290dd04daa288023b1e3f7dd974))
* categorise offered overlays ([#62](https://github.com/teevik/tarkov-map/issues/62)) ([93b90a1](https://github.com/teevik/tarkov-map/commit/93b90a1b77e3e8000b09c323ed1ba51efb564b80))
* make map refreshes traceable and atomic ([#20](https://github.com/teevik/tarkov-map/issues/20)) ([2d0987e](https://github.com/teevik/tarkov-map/commit/2d0987e72661a6dd5507326c87c65ae80f08baac))
* share label placement across overlays ([#63](https://github.com/teevik/tarkov-map/issues/63)) ([9e548e9](https://github.com/teevik/tarkov-map/commit/9e548e98af2a92d3719f7351244df51a05779070))
* validate bundled map collection ([#19](https://github.com/teevik/tarkov-map/issues/19)) ([9fd518c](https://github.com/teevik/tarkov-map/commit/9fd518c5df587f6186ecbe03c1f370f865b541c7))


### Bug Fixes

* borrow map for overlay sidebar ([#62](https://github.com/teevik/tarkov-map/issues/62)) ([ea38c78](https://github.com/teevik/tarkov-map/commit/ea38c78caae2c9be302d008600de1618b2f08191))

## [0.1.14](https://github.com/teevik/tarkov-map/compare/v0.1.13...v0.1.14) (2026-08-17)


### Features

* bound retained map texture memory with a budgeted LRU ([fcf612b](https://github.com/teevik/tarkov-map/commit/fcf612baa1c018e6ce784ee3066f2017d1516bda)), closes [#15](https://github.com/teevik/tarkov-map/issues/15)
* load map images on demand instead of preloading all ([59c2fc8](https://github.com/teevik/tarkov-map/commit/59c2fc89c25a676d3169e733632bcd81db1d2ac2)), closes [#14](https://github.com/teevik/tarkov-map/issues/14)
* ship GPU-compressed BC7 map textures with active-image-only retention ([baf6b77](https://github.com/teevik/tarkov-map/commit/baf6b774fb54635f55cdf41317953c88d317ba16)), closes [#24](https://github.com/teevik/tarkov-map/issues/24)
* update all dependencies, migrate to eframe/egui 0.36 ([32f1f9e](https://github.com/teevik/tarkov-map/commit/32f1f9e6656faa7840f21dfa83b1f61dc3f32da9)), closes [#23](https://github.com/teevik/tarkov-map/issues/23)

## [0.1.13](https://github.com/teevik/tarkov-map/compare/v0.1.12...v0.1.13) (2026-08-17)


### Bug Fixes

* maps ([d5b72cd](https://github.com/teevik/tarkov-map/commit/d5b72cd80bbd6b43106cbdcb4e79b496159c54d3))

## [0.1.12](https://github.com/teevik/tarkov-map/compare/v0.1.11...v0.1.12) (2026-08-09)


### Features

* update maps ([9d9a62d](https://github.com/teevik/tarkov-map/commit/9d9a62d3abdbefd0174ba39e507f55fea22182e6))

## [0.1.11](https://github.com/teevik/tarkov-map/compare/v0.1.10...v0.1.11) (2026-03-06)


### Bug Fixes

* trigger an empty patch release ([f81e349](https://github.com/teevik/tarkov-map/commit/f81e34901a41d2c65312f0c9599fc3208abb4b0a))

## [0.1.10](https://github.com/teevik/tarkov-map/compare/v0.1.9...v0.1.10) (2026-01-13)


### Features

* window title ([8ec5c38](https://github.com/teevik/tarkov-map/commit/8ec5c3882f12f96c55080530fcf3b59ea894c05e))

## [0.1.9](https://github.com/teevik/tarkov-map/compare/v0.1.8...v0.1.9) (2026-01-13)


### Features

* github link ([da80a4a](https://github.com/teevik/tarkov-map/commit/da80a4a0f65c37d9a0603f3061f93c603e1ca758))
* hide border when fullscreened, and proper windows decorations ([994b14f](https://github.com/teevik/tarkov-map/commit/994b14fa58432fa1629528d816238ad1b942eda0))

## [0.1.8](https://github.com/teevik/tarkov-map/compare/v0.1.7...v0.1.8) (2026-01-13)


### Features

* add CI and nix caching ([0ef8c24](https://github.com/teevik/tarkov-map/commit/0ef8c24f2de40fac3a594b55d85f22464e139aff))
* add license ([19caa48](https://github.com/teevik/tarkov-map/commit/19caa48dfffbdd81df2da8e212be9fcb87016793))
* app icon ([25a24d4](https://github.com/teevik/tarkov-map/commit/25a24d4925a3f73d68cd6adef64fa9fe0ea01fb8))
* Persisted sidebar settings ([89c88c0](https://github.com/teevik/tarkov-map/commit/89c88c0d756a60942d83658eb6846e8d86fce95a))
* release-please ([85568f2](https://github.com/teevik/tarkov-map/commit/85568f2a39cac1c5fe471485a0585e362f72c3f9))
* self updating with notification ([1ad7774](https://github.com/teevik/tarkov-map/commit/1ad777430d20e9b8ab5be849427764adaaffa3a6))
* self_update assets in CI ([c4c714b](https://github.com/teevik/tarkov-map/commit/c4c714b538128a868836c45a4cf55a86eac9c3a1))
* Simplify UI ([5b7769a](https://github.com/teevik/tarkov-map/commit/5b7769a1a5274a842631d5c1b1416850fe0f618f))
* tarkov screenshot position tracking ([48dd827](https://github.com/teevik/tarkov-map/commit/48dd82795724c6edfd498eb8c767951a18a9c321))
* Use PAT for release-please ([5f3dd85](https://github.com/teevik/tarkov-map/commit/5f3dd85698cd649ecbb880dc24d489d730fcf64d))
* use skip_serializing_none to simplify code ([b86a16d](https://github.com/teevik/tarkov-map/commit/b86a16d88316499a0d043b14d1dce7938f68f021))


### Bug Fixes

* correct nix build in CI ([dac0a33](https://github.com/teevik/tarkov-map/commit/dac0a3399eccfda3c3a32692888fd11bfb1b3cef))
* release tags dont get matched by updater ([f8ded02](https://github.com/teevik/tarkov-map/commit/f8ded02e7b2afe99e16a0816df4e9dcf28cc0bbc))

## [0.1.7](https://github.com/teevik/tarkov-map/compare/tarkov-map-v0.1.6...tarkov-map-v0.1.7) (2026-01-13)


### Features

* self_update assets in CI ([c4c714b](https://github.com/teevik/tarkov-map/commit/c4c714b538128a868836c45a4cf55a86eac9c3a1))

## [0.1.6](https://github.com/teevik/tarkov-map/compare/tarkov-map-v0.1.5...tarkov-map-v0.1.6) (2026-01-12)


### Features

* self updating with notification ([1ad7774](https://github.com/teevik/tarkov-map/commit/1ad777430d20e9b8ab5be849427764adaaffa3a6))

## [0.1.5](https://github.com/teevik/tarkov-map/compare/tarkov-map-v0.1.4...tarkov-map-v0.1.5) (2026-01-12)


### Features

* Persisted sidebar settings ([89c88c0](https://github.com/teevik/tarkov-map/commit/89c88c0d756a60942d83658eb6846e8d86fce95a))

## [0.1.4](https://github.com/teevik/tarkov-map/compare/tarkov-map-v0.1.3...tarkov-map-v0.1.4) (2026-01-10)


### Features

* app icon ([25a24d4](https://github.com/teevik/tarkov-map/commit/25a24d4925a3f73d68cd6adef64fa9fe0ea01fb8))
* tarkov screenshot position tracking ([48dd827](https://github.com/teevik/tarkov-map/commit/48dd82795724c6edfd498eb8c767951a18a9c321))
* use skip_serializing_none to simplify code ([b86a16d](https://github.com/teevik/tarkov-map/commit/b86a16d88316499a0d043b14d1dce7938f68f021))

## [0.1.3](https://github.com/teevik/tarkov-map/compare/tarkov-map-v0.1.2...tarkov-map-v0.1.3) (2026-01-10)


### Features

* Use PAT for release-please ([5f3dd85](https://github.com/teevik/tarkov-map/commit/5f3dd85698cd649ecbb880dc24d489d730fcf64d))

## [0.1.2](https://github.com/teevik/tarkov-map/compare/tarkov-map-v0.1.1...tarkov-map-v0.1.2) (2026-01-10)


### Features

* Simplify UI ([5b7769a](https://github.com/teevik/tarkov-map/commit/5b7769a1a5274a842631d5c1b1416850fe0f618f))

## [0.1.1](https://github.com/teevik/tarkov-map/compare/tarkov-map-v0.1.0...tarkov-map-v0.1.1) (2026-01-09)


### Features

* add CI and nix caching ([0ef8c24](https://github.com/teevik/tarkov-map/commit/0ef8c24f2de40fac3a594b55d85f22464e139aff))
* add license ([19caa48](https://github.com/teevik/tarkov-map/commit/19caa48dfffbdd81df2da8e212be9fcb87016793))
* release-please ([85568f2](https://github.com/teevik/tarkov-map/commit/85568f2a39cac1c5fe471485a0585e362f72c3f9))


### Bug Fixes

* correct nix build in CI ([dac0a33](https://github.com/teevik/tarkov-map/commit/dac0a3399eccfda3c3a32692888fd11bfb1b3cef))
