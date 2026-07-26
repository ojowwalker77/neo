/* The .mathset file — a set of math that generates an image.
 *
 * Coordinates are normalized against the LONG edge of the canvas, so a set
 * fitted at one resolution replays at any other. Nothing in here refers to a
 * pixel. That is the whole point: the file describes the continuous thing that
 * pixels are a sampling of, not the samples.
 *
 *   canvas [W,H]  reference resolution — a hint for framing, not a constraint
 *   bg            the canvas before any primitive is composited
 *   splats        ORDERED, back to front. Alpha compositing does not commute,
 *                 so this is a sequence, not a set.
 *
 * Each splat is 9 or 10 numbers:
 *   x y           centre, normalized
 *   sx sy         standard deviations along the primitive's own axes
 *   theta         rotation of those axes, radians, CCW
 *   r g b         colour, sRGB-encoded, 0..1
 *   a             peak opacity at the centre
 *   beta          shape exponent, optional, defaults to 1
 *
 * The falloff is exp(-0.5 * (u^2 + v^2)^beta) in the primitive's own frame.
 * beta = 1 is an ordinary Gaussian. Large beta approaches a hard-edged
 * ellipse; below 1 the tails run longer than a Gaussian's. One number spans
 * soft to sharp, which is the one thing a plain Gaussian cannot do and the
 * thing edges in real images need.
 */
use serde::{Deserialize, Serialize};

pub const VERSION: u32 = 0;
pub const GAUSS2D: &str = "gauss2d";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MathSet {
    pub mathset: u32,
    pub canvas: [u32; 2],
    #[serde(default = "srgb")]
    pub space: String,
    pub bg: [f32; 3],
    pub primitive: String,
    pub splats: Vec<Vec<f32>>,
}

fn srgb() -> String {
    "srgb".into()
}

#[derive(Debug, Clone, Copy)]
pub struct Splat {
    pub x: f32,
    pub y: f32,
    pub sx: f32,
    pub sy: f32,
    pub theta: f32,
    pub rgb: [f32; 3],
    pub a: f32,
    pub beta: f32,
}

/// How far out the footprint is worth drawing, in sigma units, for a given
/// shape exponent: where the falloff drops below half an 8-bit code value.
/// Derived, not tuned — a sharper primitive needs a tighter quad, a
/// long-tailed one needs a wider one.
pub fn envelope(beta: f32) -> f32 {
    const EPS: f32 = 1.0 / 510.0;
    (-2.0 * EPS.ln()).powf(1.0 / (2.0 * beta))
}

impl MathSet {
    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        let txt = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let ms: MathSet = serde_json::from_str(&txt).map_err(|e| format!("{}: {e}", path.display()))?;
        ms.validate()?;
        Ok(ms)
    }

    fn validate(&self) -> Result<(), String> {
        if self.mathset != VERSION {
            return Err(format!("mathset version {} — this decoder speaks {VERSION}", self.mathset));
        }
        if self.primitive != GAUSS2D {
            return Err(format!("unknown primitive {:?} — this decoder speaks {GAUSS2D:?}", self.primitive));
        }
        if self.space != "srgb" {
            return Err(format!("unknown colour space {:?}", self.space));
        }
        if self.canvas[0] == 0 || self.canvas[1] == 0 {
            return Err("canvas has a zero dimension".into());
        }
        for (i, s) in self.splats.iter().enumerate() {
            if s.len() != 9 && s.len() != 10 {
                return Err(format!("splat {i}: expected 9 or 10 numbers, found {}", s.len()));
            }
            if !s.iter().all(|v| v.is_finite()) {
                return Err(format!("splat {i}: non-finite number"));
            }
            if s[2] <= 0.0 || s[3] <= 0.0 {
                return Err(format!("splat {i}: sx and sy must be positive (got {}, {})", s[2], s[3]));
            }
            if s.len() == 10 && s[9] <= 0.0 {
                return Err(format!("splat {i}: beta must be positive (got {})", s[9]));
            }
        }
        Ok(())
    }

    /// Long edge of the reference canvas. The unit that normalized coords
    /// are measured in.
    pub fn unit(&self) -> f32 {
        self.canvas[0].max(self.canvas[1]) as f32
    }

    pub fn splat(&self, i: usize) -> Splat {
        let s = &self.splats[i];
        Splat {
            x: s[0],
            y: s[1],
            sx: s[2],
            sy: s[3],
            theta: s[4],
            rgb: [s[5], s[6], s[7]],
            a: s[8],
            beta: if s.len() == 10 { s[9] } else { 1.0 },
        }
    }

    pub fn iter_splats(&self) -> impl Iterator<Item = Splat> + '_ {
        (0..self.splats.len()).map(|i| self.splat(i))
    }
}

/// sRGB-encoded channel to linear light. Compositing happens in linear light;
/// the file stores sRGB because that is what a human reading the file expects.
pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}
