# Inspiration

Where Manfred's ported code comes from. The licences and copyright lines are in NOTICE.

- **Routes** (`crates/motion/src/route.rs`): the arc-length baking of Godot Engine's Curve3D, `scene/resources/curve.cpp` at tag 4.5-stable (commit 876b290332ec6f2e6d173d08162a02aa7e6ca46d), MIT: `_bake_segment3d_even_length`'s recursive even-length subdivision, `_find_interval`'s bisection of the distance table, and `get_closest_offset`'s projection onto the baked polyline. The curve itself is a centripetal Catmull-Rom spline in Hermite form, after Yuksel, Schaefer and Keyser, "Parameterization and applications of Catmull-Rom curves" (2011), evaluated exactly at the baked parameter rather than interpolated between baked points.
- **Noise** (`crates/motion/src/noise.rs`): 1D gradient noise with Perlin's quintic fade (Improved Noise, 2002), two octaves, hashed by the same 64-bit finaliser as `crates/generators/src/noise.rs`.
