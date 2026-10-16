# Inspiration

Where Manfred's ported code comes from. The licences and copyright lines are in NOTICE.

- **Routes** (`crates/motion/src/route.rs`): the arc-length baking of Godot Engine's Curve3D, `scene/resources/curve.cpp` at tag 4.5-stable (commit 876b290332ec6f2e6d173d08162a02aa7e6ca46d), MIT: `_bake_segment3d_even_length`'s recursive even-length subdivision, `_find_interval`'s bisection of the distance table, and `get_closest_offset`'s projection onto the baked polyline. The curve itself is a centripetal Catmull-Rom spline in Hermite form, after Yuksel, Schaefer and Keyser, "Parameterization and applications of Catmull-Rom curves" (2011), evaluated exactly at the baked parameter rather than interpolated between baked points.
- **Noise** (`crates/motion/src/noise.rs`): 1D gradient noise with Perlin's quintic fade (Improved Noise, 2002), two octaves, hashed by the same 64-bit finaliser as `crates/generators/src/noise.rs`.
- **An animated scene file in the window** (`crates/ui/src/animated.rs`): the frame loop of pfx-play's presenter, `crates/play/src/present.rs` at pfx tag v2.2.0 (commit f99be6c4eb9008b8726c339ff29cf7d752582058), MIT: the node palettes sampled from the object's clip, `Draws::models_posed` over the staged instances and glass in draw order, attached lights by `Light::placed_with`, and parented sprays by `Sprays::parents` and `seek_between`.

| Manfred | Origin | Taken at | Licence |
|---|---|---|---|
| `crates/mesh/src/simplify.rs`: the levels of detail's simplifier | meshoptimizer's `src/simplifier.cpp` (edge collapses, quadric error with attribute weights, vertex kinds and seams, collapse ranking and ordering), by Arseny Kapoulkine, https://github.com/zeux/meshoptimizer | 0.25, as vendored by the meshopt 0.6.2 crate | MIT |
| `crates/mesh/src/tangents.rs`: tangents for the baked normal maps | bevy_mikktspace (MikkTSpace in Rust), https://github.com/bevyengine/bevy_mikktspace, after Morten S. Mikkelsen's MikkTSpace, https://github.com/mmikk/MikkTSpace | 1.0.0 | MIT or Apache-2.0, and zlib |
