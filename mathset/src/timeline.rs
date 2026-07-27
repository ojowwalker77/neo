/* A keyframed timeline of one persistent ordered mathset.
 *
 * The frame files are not independent fits: each is warm-started from the
 * previous state without changing row count or order. The timeline records
 * those states and samples between adjacent ones with parameter-aware
 * interpolation.
 */
use crate::format::MathSet;
use crate::transition;
use serde::Deserialize;

const VERSION: u32 = 0;

#[derive(Debug, Deserialize)]
pub struct Timeline {
    pub timeline: u32,
    pub fps: f32,
    pub frames: Vec<String>,
}

impl Timeline {
    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        let text =
            std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let timeline: Self =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        if timeline.timeline != VERSION {
            return Err(format!(
                "{}: timeline version {} — this sampler speaks {VERSION}",
                path.display(),
                timeline.timeline
            ));
        }
        if !timeline.fps.is_finite() || timeline.fps <= 0.0 {
            return Err(format!("{}: fps must be positive and finite", path.display()));
        }
        if timeline.frames.len() < 2 {
            return Err(format!("{}: timeline needs at least two frames", path.display()));
        }
        if timeline.frames.iter().any(|frame| frame.is_empty()) {
            return Err(format!("{}: timeline contains an empty frame path", path.display()));
        }
        Ok(timeline)
    }

    pub fn sample(
        &self,
        descriptor_path: &std::path::Path,
        t: f32,
    ) -> Result<(MathSet, usize, usize, f32), String> {
        if !t.is_finite() || !(0.0..=1.0).contains(&t) {
            return Err("--t must be finite and between 0 and 1".into());
        }
        let position = t * (self.frames.len() - 1) as f32;
        let left = (position.floor() as usize).min(self.frames.len() - 1);
        let right = (left + 1).min(self.frames.len() - 1);
        let local_t = position - left as f32;
        let base = descriptor_path.parent().unwrap_or(std::path::Path::new("."));
        let a = MathSet::load(&base.join(&self.frames[left]))?;
        if left == right {
            return Ok((a, left, right, 0.0));
        }
        let b = MathSet::load(&base.join(&self.frames[right]))?;
        let (out, _) = transition::between_states(&a, &b, local_t)?;
        Ok((out, left, right, local_t))
    }
}
