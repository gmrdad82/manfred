# Manfred's noise

`manfred_generators::noise` (`manfred::generators::noise` on the facade) is Manfred's own gradient noise, for an app's generators that once shaped their meshes with Blender's `mathutils.noise.noise()`; Manfred does not port Blender's code. It builds the function from the algorithm as the literature describes it.

## Calls

- `perlin([x, y, z]) -> f64`: one lattice cell per unit, mean 0, standard deviation 0.278, range about −0.96 to 0.95.
- `fractal(p, octaves)`: the fractal sum a typical Blender generator script computes over `noise.noise()`: amplitude 0.5 halving each octave, frequency growing by 2.03 each octave, summed over `perlin`.
- `fractal_with(p, octaves, gain, lacunarity)`: the same with the two factors given. `FRACTAL_GAIN` is 0.5 and `FRACTAL_LACUNARITY` is 2.03.

## Construction

1. **Lattice.** `floor` of each coordinate picks the cell, and the fractions `x, y, z` in [0, 1) place the point inside it. Each of the 8 corners owns one gradient direction.
2. **Hash.** A corner's cell coordinates are multiplied by three odd 64-bit constants, added to a fixed seed, and passed through a 64-bit finalizer of two xor-shift and multiply rounds. The top 32 bits choose a gradient from the table by multiply-and-shift. Only integer arithmetic is involved.
3. **Gradients.** The table is built at compile time by a `const fn` from the same hash over a counter. It draws integer vectors in a cube, keeps those with a length between half the cube's half-width and the half-width, and normalises them with a Newton square root, so they are uniform on the sphere to within the rejection. Each of 32 such vectors is expanded into 24: its 3 cyclic permutations of components, times all 8 sign combinations. The 768 gradients are unit, come in opposite pairs, fill every octant equally, and have exactly equal second moments on the three axes with zero cross terms, so no axis is special. The tests check each of these.
4. **Corner contribution.** The dot product of the corner's gradient with the vector from the corner to the point.
5. **Fade.** `6t⁵ − 15t⁴ + 10t³` on each fraction, which has zero first and second derivatives at the cell walls, so the result is continuous and its slope is too.
6. **Blend.** Trilinear interpolation of the 8 contributions with the faded fractions.
7. **Scale.** The result is multiplied by 1.456.

At a lattice point every fraction is 0, so every dot product with a zero vector is 0: `perlin` is exactly 0 on integer coordinates, as Blender's is.

## What was matched, and measured

Blender 5's `noise.noise(Vector)` was run headless on 400,000 random points in a 100-unit cube, and the same four-octave `fractal(p, 4)` in Python on 100,000:

| | mean | standard deviation | range |
|---|---|---|---|
| Blender `noise` | 0.0001 | 0.2778 | −0.99 to 0.96 |
| Blender `fractal(p, 4)` | 0.0003 | 0.1596 | −0.65 to 0.65 |
| Manfred `perlin` (1,000,000 points) | 0.0003 | 0.2780 | −0.96 to 0.95 |

The unscaled function has a deviation of 0.1908, so the 1.456 factor brings it to Blender's. With one lattice cell per unit and the same spread, a script's multipliers (`p * 260.0`, `p3 * 90.0` and the like) give detail at the same scale in Manfred as in Blender. The values at a given point differ from Blender's, so every surface shaped by it looks the same in kind and different in detail; that is the re-roll.

## Properties and tests

The tests in `noise.rs` check: determinism; continuity, in small steps and across cell walls; zero at lattice points; the mean and spread over a million points against Blender's figures (within 20%); the fractal's spread against Blender's four-octave figure; the same correlation at a quarter-unit step along x, y and z; the same slope in the three axes and the diagonals; negative and huge coordinates. `perlin` runs at about 60 million calls a second on one core in release (the test asserts 20 million; it is `#[ignore]` since it reads a clock: `cargo test --release -p manfred-generators noise -- --ignored`). The slice test writes `tmp/noise.png`, 512² tiles at 4, 16, 64 and 260 cells across.

The results are portable: only `floor`, add, subtract, multiply and the integer hash are in the hot path, and the table's square roots are computed by const arithmetic.
