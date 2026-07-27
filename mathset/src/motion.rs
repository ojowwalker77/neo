/* Stage 5: recover rigid motion across two frames.
 *
 * Per-primitive gradients have a local capture radius. This module works above
 * that level: find coherent changed regions in the source frames, estimate one
 * translation per region from frame correspondence, then move every primitive
 * in the region as a unit. Ordinary refinement takes over once the primitives
 * have crossed the large-motion basin.
 */
use crate::fit;
use crate::format::MathSet;
use crate::render;

#[derive(Clone, Copy)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn parse(v: &str) -> Result<Self, String> {
        let values: Vec<f32> = v
            .split(',')
            .map(|n| n.trim().parse().map_err(|_| format!("--rect: {n:?}")))
            .collect::<Result<_, _>>()?;
        if values.len() != 4 {
            return Err(format!("--rect wants X,Y,W,H (got {v:?})"));
        }
        if !values.iter().all(|n| n.is_finite()) {
            return Err("--rect values must be finite".into());
        }
        if values[2] <= 0.0 || values[3] <= 0.0 {
            return Err("--rect width and height must be positive".into());
        }
        Ok(Rect {
            x: values[0],
            y: values[1],
            w: values[2],
            h: values[3],
        })
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        self.x <= x && x <= self.x + self.w && self.y <= y && y <= self.y + self.h
    }
}

#[derive(Clone, Copy)]
pub struct Options {
    pub range: f32,
    pub levels: u32,
    pub max_side: u32,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            range: 0.1,
            levels: 5,
            max_side: 128,
        }
    }
}

pub fn track_group(
    img: &image::DynamicImage,
    ms: &MathSet,
    rect: Rect,
    output: &std::path::Path,
    opt: Options,
) -> Result<(), String> {
    let members: Vec<usize> = ms
        .iter_splats()
        .enumerate()
        .filter(|(_, s)| rect.contains(s.x, s.y))
        .map(|(i, _)| i)
        .collect();
    if members.is_empty() {
        return Err("--rect contains no primitive centres".into());
    }
    track_members(img, ms, &members, output, opt)
}

/// Search a rigid translation for one known group against the primitive
/// reconstruction. This isolates whether group motion can cross the basin;
/// automatic discovery uses source-frame correspondence below.
fn track_members(
    img: &image::DynamicImage,
    ms: &MathSet,
    members: &[usize],
    output: &std::path::Path,
    opt: Options,
) -> Result<(), String> {
    let (w, h) = render::working_size(ms.canvas, Some(opt.max_side));
    let target = img
        .resize_exact(w, h, image::imageops::FilterType::Lanczos3)
        .to_rgba8()
        .as_raw()
        .clone();
    let gpu = render::Gpu::new()?;
    let shifted = |dx: f32, dy: f32| {
        let mut candidate = ms.clone();
        for &i in members {
            candidate.splats[i][0] += dx;
            candidate.splats[i][1] += dy;
        }
        candidate
    };
    let score = |set: &MathSet| -> Result<f64, String> {
        Ok(fit::psnr(
            &target,
            &render::render_with(&gpu, set, w, h, None)?,
        ))
    };

    let mut best = (0.0f32, 0.0f32);
    let mut best_db = score(ms)?;
    let start_db = best_db;
    let mut step = opt.range / 2.0;
    let start = std::time::Instant::now();
    for level in 0..opt.levels {
        let radius = if level == 0 { 2 } else { 1 };
        let centre = best;
        for gy in -radius..=radius {
            for gx in -radius..=radius {
                let dx = centre.0 + gx as f32 * step;
                let dy = centre.1 + gy as f32 * step;
                if dx.abs() > opt.range || dy.abs() > opt.range {
                    continue;
                }
                let db = score(&shifted(dx, dy))?;
                if db > best_db {
                    best = (dx, dy);
                    best_db = db;
                }
            }
        }
        println!(
            "  level {}/{} · step {:.3} px · best {:+.2},{:+.2} px · {best_db:.2} dB",
            level + 1,
            opt.levels,
            step * ms.canvas[0].max(ms.canvas[1]) as f32,
            best.0 * ms.canvas[0].max(ms.canvas[1]) as f32,
            best.1 * ms.canvas[0].max(ms.canvas[1]) as f32,
        );
        step /= 2.0;
    }

    let out = shifted(best.0, best.1);
    out.save(output)?;
    let saved = MathSet::load(output)?;
    let saved_db = score(&saved)?;
    let unit = ms.canvas[0].max(ms.canvas[1]) as f32;
    println!(
        "{} primitives in group · search {}x{} · {start_db:.2} -> {saved_db:.2} dB · {:.1}s",
        members.len(),
        w,
        h,
        start.elapsed().as_secs_f64()
    );
    println!(
        "translation {:+.5},{:+.5} normalized = {:+.2},{:+.2} reference px",
        best.0,
        best.1,
        best.0 * unit,
        best.1 * unit
    );
    println!("-> {}", output.display());
    Ok(())
}

fn frame_translation(
    a: &image::RgbImage,
    b: &image::RgbImage,
    bounds: (u32, u32, u32, u32),
    unit: f32,
    opt: Options,
) -> Result<(f32, f32, f64), String> {
    let (x0, y0, x1, y1) = bounds;
    let full_samples = ((x1 - x0 + 1) * (y1 - y0 + 1) * 3) as usize;
    let cost = |dx: f32, dy: f32| -> Option<f64> {
        let mut se = 0.0f64;
        let mut n = 0usize;
        for y in y0..=y1 {
            for x in x0..=x1 {
                let tx = x as f32 + dx;
                let ty = y as f32 + dy;
                // Keep both samples inside this change component. Bilinear
                // interpolation also needs the pixel immediately right/below.
                if tx < x0 as f32 || tx >= x1 as f32 || ty < y0 as f32 || ty >= y1 as f32 {
                    continue;
                }
                let ix = tx.floor() as u32;
                let iy = ty.floor() as u32;
                let fx = (tx - ix as f32) as f64;
                let fy = (ty - iy as f32) as f64;
                let src = a.get_pixel(x, y).0;
                let p00 = b.get_pixel(ix, iy).0;
                let p10 = b.get_pixel(ix + 1, iy).0;
                let p01 = b.get_pixel(ix, iy + 1).0;
                let p11 = b.get_pixel(ix + 1, iy + 1).0;
                for c in 0..3 {
                    let top = p00[c] as f64 * (1.0 - fx) + p10[c] as f64 * fx;
                    let bottom = p01[c] as f64 * (1.0 - fx) + p11[c] as f64 * fx;
                    let sample = top * (1.0 - fy) + bottom * fy;
                    let d = src[c] as f64 - sample;
                    se += d * d;
                    n += 1;
                }
            }
        }
        (n > 0 && n >= full_samples / 4).then_some(se / n as f64)
    };

    let radius = (opt.range * unit).ceil() as i32;
    let mut best = (f64::INFINITY, 0.0f32, 0.0f32);
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if let Some(c) = cost(dx as f32, dy as f32)
                && c < best.0
            {
                best = (c, dx as f32, dy as f32);
            }
        }
    }
    if !best.0.is_finite() {
        return Err("change component has no searchable pixel overlap".into());
    }

    let mut step = 0.5f32;
    for _ in 1..opt.levels {
        let centre = best;
        for gy in -1..=1 {
            for gx in -1..=1 {
                let dx = centre.1 + gx as f32 * step;
                let dy = centre.2 + gy as f32 * step;
                if dx.abs() > opt.range * unit || dy.abs() > opt.range * unit {
                    continue;
                }
                if let Some(c) = cost(dx, dy)
                    && c < best.0
                {
                    best = (c, dx, dy);
                }
            }
        }
        step /= 2.0;
    }
    Ok((best.1, best.2, best.0))
}

pub fn track_changes(
    a: &image::DynamicImage,
    b: &image::DynamicImage,
    ms: &MathSet,
    output: &std::path::Path,
    threshold: u8,
    opt: Options,
) -> Result<(), String> {
    let (w, h) = render::working_size(ms.canvas, Some(opt.max_side));
    let ar = a
        .resize_exact(w, h, image::imageops::FilterType::Lanczos3)
        .to_rgb8();
    let br = b
        .resize_exact(w, h, image::imageops::FilterType::Lanczos3)
        .to_rgb8();

    const CELL: u32 = 8;
    const MIN_CHANGED: usize = 8;
    let cells_x = w.div_ceil(CELL);
    let cells_y = h.div_ceil(CELL);
    let mut cells: Vec<Vec<(u32, u32)>> = vec![Vec::new(); (cells_x * cells_y) as usize];
    for y in 0..h {
        for x in 0..w {
            let pa = ar.get_pixel(x, y).0;
            let pb = br.get_pixel(x, y).0;
            if (0..3).map(|c| pa[c].abs_diff(pb[c])).max().unwrap_or(0) < threshold {
                continue;
            }
            cells[((y / CELL) * cells_x + x / CELL) as usize].push((x, y));
        }
    }

    // Texture leaves holes in a raw difference mask. Join through coarse
    // 8-connected cells so one textured surface stays one component while
    // spatially separate motions remain independent.
    let mut active: Vec<bool> = cells.iter().map(|p| !p.is_empty()).collect();
    let mut components: Vec<(usize, (u32, u32, u32, u32))> = Vec::new();
    for seed in 0..active.len() {
        if !active[seed] {
            continue;
        }
        active[seed] = false;
        let mut stack = vec![seed as u32];
        let mut count = 0usize;
        let mut bounds: Option<(u32, u32, u32, u32)> = None;
        while let Some(cell) = stack.pop() {
            for &(x, y) in &cells[cell as usize] {
                count += 1;
                bounds = Some(match bounds {
                    None => (x, y, x, y),
                    Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
                });
            }
            let cx = (cell % cells_x) as i32;
            let cy = (cell / cells_x) as i32;
            for oy in -1..=1 {
                for ox in -1..=1 {
                    let nx = cx + ox;
                    let ny = cy + oy;
                    if nx < 0 || ny < 0 || nx >= cells_x as i32 || ny >= cells_y as i32 {
                        continue;
                    }
                    let next = (ny as u32 * cells_x + nx as u32) as usize;
                    if active[next] {
                        active[next] = false;
                        stack.push(next as u32);
                    }
                }
            }
        }
        if count >= MIN_CHANGED {
            components.push((count, bounds.unwrap()));
        }
    }
    if components.is_empty() {
        return Err(format!("no frame change reaches threshold {threshold}"));
    }
    components.sort_by_key(|component| std::cmp::Reverse(component.0));

    let unit = render::unit_for(ms.canvas, w, h);
    let ref_unit = ms.canvas[0].max(ms.canvas[1]) as f32;
    let mut out = ms.clone();
    let mut assigned = vec![false; ms.splats.len()];
    let mut moved = 0usize;
    let start = std::time::Instant::now();
    for (component, (changed, bounds)) in components.into_iter().enumerate() {
        let (x0, y0, x1, y1) = bounds;
        let rect = Rect {
            x: x0 as f32 / unit,
            y: y0 as f32 / unit,
            w: (x1 + 1 - x0) as f32 / unit,
            h: (y1 + 1 - y0) as f32 / unit,
        };
        let members: Vec<usize> = ms
            .iter_splats()
            .enumerate()
            .filter(|(i, s)| !assigned[*i] && rect.contains(s.x, s.y))
            .map(|(i, _)| i)
            .collect();
        if members.is_empty() {
            continue;
        }
        let (dx, dy, cost) = frame_translation(&ar, &br, bounds, unit, opt)?;
        for &i in &members {
            assigned[i] = true;
            out.splats[i][0] += dx / unit;
            out.splats[i][1] += dy / unit;
        }
        moved += members.len();
        println!(
            "  component {} · {changed} changed px · {} primitives · region x {:.1}..{:.1}, y {:.1}..{:.1} px",
            component + 1,
            members.len(),
            rect.x * ref_unit,
            (rect.x + rect.w) * ref_unit,
            rect.y * ref_unit,
            (rect.y + rect.h) * ref_unit
        );
        println!(
            "    frame match {:+.3},{:+.3} working px = {:+.2},{:+.2} reference px · mse {cost:.2}",
            dx,
            dy,
            dx / unit * ref_unit,
            dy / unit * ref_unit
        );
    }
    if moved == 0 {
        return Err("changed components contain no primitive centres".into());
    }

    out.save(output)?;
    let saved = MathSet::load(output)?;
    let target = b
        .resize_exact(w, h, image::imageops::FilterType::Lanczos3)
        .to_rgba8()
        .as_raw()
        .clone();
    let gpu = render::Gpu::new()?;
    let score = |set: &MathSet| -> Result<f64, String> {
        Ok(fit::psnr(
            &target,
            &render::render_with(&gpu, set, w, h, None)?,
        ))
    };
    let before = score(ms)?;
    let after = score(&saved)?;
    println!(
        "{moved} primitives across {}x{} frame search · {before:.2} -> {after:.2} dB · {:.1}s",
        w,
        h,
        start.elapsed().as_secs_f64()
    );
    println!("-> {}", output.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Options, frame_translation};
    use image::{Rgb, RgbImage};

    #[test]
    fn frame_correspondence_recovers_translation_beyond_gradient_radius() {
        let a = RgbImage::from_fn(32, 32, |x, y| {
            Rgb([
                ((x * 17 + y * 3) % 251) as u8,
                ((x * 5 + y * 19) % 241) as u8,
                ((x * 11 + y * 7) % 239) as u8,
            ])
        });
        let mut b = a.clone();
        let bounds = (4, 4, 27, 27);
        let shift = (3i32, -2i32);
        for y in bounds.1..=bounds.3 {
            for x in bounds.0..=bounds.2 {
                let sx = x as i32 - shift.0;
                let sy = y as i32 - shift.1;
                if sx >= bounds.0 as i32
                    && sx <= bounds.2 as i32
                    && sy >= bounds.1 as i32
                    && sy <= bounds.3 as i32
                {
                    b.put_pixel(x, y, *a.get_pixel(sx as u32, sy as u32));
                }
            }
        }

        let (dx, dy, _) = frame_translation(
            &a,
            &b,
            bounds,
            32.0,
            Options {
                range: 0.25,
                levels: 6,
                max_side: 32,
            },
        )
        .unwrap();
        assert!((dx - shift.0 as f32).abs() < 0.2, "{dx}");
        assert!((dy - shift.1 as f32).abs() < 0.2, "{dy}");
    }
}
